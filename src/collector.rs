//! One PLC, end to end.

use crate::browser::OpcUaNodeBrowser;
use crate::connection::{connect, ConnectOptions};
use crate::error::{Error, Result};
use crate::filter::{DefaultNodeFilter, NodeFilter};
use crate::monitor::{spawn_watchdog, MonitorOptions, Poller, Subscriber};
use crate::scanner::{ScanOptions, TreeScanner};
use crate::session::{OpcUaSession, PlcSession};
use crate::sink::TagSink;
use crate::tag::PlcTag;
use crate::tagset::{glob_match, TagSet};
use crate::writer::TagClient;
use opcua::client::prelude::{NodeId, Session};
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

#[cfg(feature = "json-cache")]
use crate::repository::TagRepository;

type Registry = Arc<Mutex<Vec<(String, Arc<dyn PlcSession>)>>>;
type Selector = Box<dyn Fn(&PlcTag) -> bool + Send + Sync>;
/// The raw `opcua` session and this crate's wrapper over it, from one connect.
type OpenSession = (Arc<opcua::sync::RwLock<Session>>, Arc<dyn PlcSession>);

/// Connects to one PLC, discovers its tags, and streams every change to a sink.
///
/// Subscribes as many tags as the server will monitor, polls the rest, and
/// reconnects on its own when the link drops.
///
/// ```no_run
/// use opcua_tag_browser::Collector;
///
/// # fn main() -> opcua_tag_browser::Result<()> {
/// Collector::new("line1", "opc.tcp://192.168.201.2:4840")
///     .insecure()
///     .matching("Machine/Axis*")
///     .jsonl("logs")
///     .handle_ctrl_c()
///     .run()
/// # }
/// ```
pub struct Collector {
    name: String,
    endpoint_url: String,
    sink: Option<Arc<dyn TagSink>>,
    #[cfg(feature = "json-cache")]
    cache: Option<Arc<dyn TagRepository + Send + Sync>>,
    filter: Arc<dyn NodeFilter + Send + Sync>,
    selector: Option<Selector>,
    scan_options: ScanOptions,
    connect_options: ConnectOptions,
    monitor_options: MonitorOptions,
    force_rescan: bool,
    deferred_error: Option<Error>,
    stop: Arc<AtomicBool>,
    sessions: Registry,
}

impl Collector {
    /// Creates a collector for one endpoint.
    ///
    /// Tags are cached in `plc_tags_<name>.json` by default; override with
    /// [`cache_file`](Self::cache_file) or disable with [`no_cache`](Self::no_cache).
    pub fn new(name: impl Into<String>, endpoint_url: impl Into<String>) -> Self {
        let name = name.into();

        Self {
            #[cfg(feature = "json-cache")]
            cache: Some(Arc::new(crate::repository::JsonFileTagRepository::new(
                format!("plc_tags_{name}.json"),
            ))),
            endpoint_url: endpoint_url.into(),
            name,
            sink: None,
            filter: Arc::new(DefaultNodeFilter),
            selector: None,
            scan_options: ScanOptions::default(),
            connect_options: ConnectOptions::default(),
            monitor_options: MonitorOptions::default(),
            force_rescan: false,
            deferred_error: None,
            stop: Arc::new(AtomicBool::new(false)),
            sessions: Arc::new(Mutex::new(Vec::new())),
        }
    }

    // ---- sinks -----------------------------------------------------------

    /// Sets where observed changes are delivered. Required unless
    /// [`jsonl`](Self::jsonl) is used.
    ///
    /// A closure taking `&TagChange` works directly.
    pub fn sink(mut self, sink: impl TagSink + 'static) -> Self {
        self.sink = Some(Arc::new(sink));
        self
    }

    /// Sets a sink shared with other collectors.
    pub fn shared_sink(mut self, sink: Arc<dyn TagSink>) -> Self {
        self.sink = Some(sink);
        self
    }

    /// Writes changes to daily JSONL files in `dir`.
    ///
    /// A directory error is deferred to [`run`](Self::run) so the chain reads
    /// cleanly.
    #[cfg(feature = "jsonl-sink")]
    pub fn jsonl(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        match crate::sinks::JsonlSink::new(dir) {
            Ok(sink) => self.sink = Some(Arc::new(sink)),
            Err(e) => self.deferred_error = Some(Error::SinkSetup(e.to_string())),
        }
        self
    }

    // ---- tag selection ---------------------------------------------------

    /// Monitors only the named tags, by browse path or display name.
    ///
    /// Selecting fewer tags than the server's monitored-item ceiling means
    /// everything arrives by subscription and polling never engages.
    pub fn only(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let wanted: Vec<String> = names.into_iter().map(Into::into).collect();
        self.selector = Some(Box::new(move |tag| {
            wanted.iter().any(|w| w == &tag.path || w == &tag.display_name)
        }));
        self
    }

    /// Monitors only tags whose browse path matches a glob.
    ///
    /// `*` matches any run of characters, `?` matches one.
    pub fn matching(mut self, pattern: impl Into<String>) -> Self {
        let pattern = pattern.into();
        self.selector = Some(Box::new(move |tag| glob_match(&pattern, &tag.path)));
        self
    }

    /// Monitors only tags satisfying a predicate.
    pub fn select(mut self, predicate: impl Fn(&PlcTag) -> bool + Send + Sync + 'static) -> Self {
        self.selector = Some(Box::new(predicate));
        self
    }

    // ---- connection ------------------------------------------------------

    /// Connects without encryption or authentication, trusting any certificate.
    ///
    /// Shorthand for `connect_options(ConnectOptions::insecure())`. Appropriate
    /// on an isolated machine network with no PKI; naming it keeps the choice
    /// visible in your source.
    pub fn insecure(mut self) -> Self {
        self.connect_options = ConnectOptions::insecure();
        self
    }

    /// Overrides connection and security settings.
    pub fn connect_options(mut self, options: ConnectOptions) -> Self {
        self.connect_options = options;
        self
    }

    // ---- discovery -------------------------------------------------------

    /// Caches the scanned tag list in a JSON file.
    #[cfg(feature = "json-cache")]
    pub fn cache_file(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.cache = Some(Arc::new(crate::repository::JsonFileTagRepository::new(
            path,
        )));
        self
    }

    /// Caches the scanned tag list somewhere of your choosing.
    #[cfg(feature = "json-cache")]
    pub fn cache(mut self, repo: Arc<dyn TagRepository + Send + Sync>) -> Self {
        self.cache = Some(repo);
        self
    }

    /// Disables caching; every start rescans the address space.
    #[cfg(feature = "json-cache")]
    pub fn no_cache(mut self) -> Self {
        self.cache = None;
        self
    }

    /// Ignores any cached tag list and rescans once.
    pub fn force_rescan(mut self, yes: bool) -> Self {
        self.force_rescan = yes;
        self
    }

    /// Replaces the node filter applied during scanning.
    pub fn filter(mut self, filter: Arc<dyn NodeFilter + Send + Sync>) -> Self {
        self.filter = filter;
        self
    }

    /// Overrides scan tuning.
    pub fn scan_options(mut self, options: ScanOptions) -> Self {
        self.scan_options = options;
        self
    }

    /// Overrides subscription and polling tuning.
    pub fn monitor_options(mut self, options: MonitorOptions) -> Self {
        self.monitor_options = options;
        self
    }

    // ---- lifecycle -------------------------------------------------------

    /// Returns a handle for stopping this collector from another thread.
    ///
    /// Take it before [`run`](Self::run), which consumes the collector.
    pub fn handle(&self) -> CollectorHandle {
        CollectorHandle {
            stop: self.stop.clone(),
            sessions: self.sessions.clone(),
        }
    }

    /// Stops this collector cleanly on Ctrl+C.
    ///
    /// Installs a process-wide signal handler, so call it from a binary that
    /// owns the process. Inside a larger application, take a
    /// [`handle`](Self::handle) and wire it into your own instead.
    #[cfg(feature = "ctrl-c")]
    pub fn handle_ctrl_c(self) -> Self {
        let handle = self.handle();
        if let Err(e) = ctrlc::set_handler(move || handle.stop()) {
            log::warn!("could not install Ctrl+C handler: {e}");
        }
        self
    }

    /// Scans the address space and returns the tags, without monitoring them.
    pub fn discover(&self) -> Result<TagSet> {
        if let Some(e) = &self.deferred_error {
            return Err(Error::SinkSetup(e.to_string()));
        }
        let (_, session) = self.open("discover")?;
        self.load_or_scan(session)
    }

    /// Opens a session for reading and writing individual tags.
    ///
    /// The returned client is independent of [`run`](Self::run) and usable on
    /// its own.
    ///
    /// ```no_run
    /// # use opcua_tag_browser::Collector;
    /// # fn main() -> opcua_tag_browser::Result<()> {
    /// let plc = Collector::new("line1", "opc.tcp://192.168.201.2:4840")
    ///     .insecure()
    ///     .client()?;
    ///
    /// plc.set("Machine/Axis1/Setpoint", 1500.0)?;
    /// println!("{}", plc.get("Machine/Axis1/Speed")?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn client(&self) -> Result<TagClient> {
        let (_, session) = self.open(&format!("{}-client", self.name))?;
        let tags = self.load_or_scan(session.clone())?;
        Ok(TagClient::new(session, Arc::new(tags)))
    }

    /// Runs until stopped. Blocks the calling thread.
    pub fn run(mut self) -> Result<()> {
        if let Some(e) = self.deferred_error.take() {
            return Err(e);
        }

        let sink = self.sink.clone().ok_or(Error::MissingSink)?;

        let (raw, session) = self.open(&self.name)?;
        let _guard = StopOnDrop(self.stop.clone());

        spawn_watchdog(
            self.name.clone(),
            self.endpoint_url.clone(),
            session.clone(),
            sink.clone(),
            self.stop.clone(),
            self.monitor_options.health_check_interval,
        );

        let tags = self.selected(self.load_or_scan(session)?);
        if tags.is_empty() {
            log::warn!("[{}] no tags to monitor", self.name);
            return Ok(());
        }

        // Subscriptions are cheap per update but capped by the server.
        // Everything above the cap falls through to polling.
        let split_at = self.monitor_options.max_subscribed_items.min(tags.len());
        let (to_subscribe, remainder) = tags.split_at(split_at);

        log::info!(
            "[{}] subscribing {} tags, polling {}",
            self.name,
            to_subscribe.len(),
            remainder.len()
        );

        let subscriber = Subscriber::new(
            raw.clone(),
            self.monitor_options.clone(),
            self.name.clone(),
            sink.clone(),
        );
        let rejected = subscriber.subscribe_all(to_subscribe);

        let mut to_poll = remainder.to_vec();
        to_poll.extend(rejected);
        self.spawn_poller(to_poll, sink);

        log::info!("[{}] listening", self.name);

        // `Session::run` owns this session; the poller has its own.
        let outcome = panic::catch_unwind(AssertUnwindSafe(|| Session::run(raw)));
        if let Err(payload) = outcome {
            return Err(Error::InternalPanic(crate::session::panic_message(&payload)));
        }

        Ok(())
    }

    // ---- internals -------------------------------------------------------

    /// Applies the selector, if one was set.
    fn selected(&self, tags: TagSet) -> Vec<PlcTag> {
        match &self.selector {
            None => tags.into_vec(),
            Some(predicate) => {
                let all = tags.len();
                let kept: Vec<PlcTag> =
                    tags.into_vec().into_iter().filter(|t| predicate(t)).collect();
                log::info!("[{}] selected {} of {all} tags", self.name, kept.len());
                kept
            }
        }
    }

    /// Opens a session and registers it for shutdown.
    fn open(&self, name: &str) -> Result<OpenSession> {
        self.close_previous(name);

        let raw = connect(&self.endpoint_url, &self.connect_options)?;
        let session: Arc<dyn PlcSession> = Arc::new(OpcUaSession::new(raw.clone()));

        registry_lock(&self.sessions).push((name.to_string(), session.clone()));

        Ok((raw, session))
    }

    /// Closes any session already registered under `name`.
    fn close_previous(&self, name: &str) {
        let stale: Vec<Arc<dyn PlcSession>> = {
            let mut sessions = registry_lock(&self.sessions);
            let mut stale = Vec::new();
            sessions.retain(|(existing, session)| {
                if existing == name {
                    stale.push(session.clone());
                    false
                } else {
                    true
                }
            });
            stale
        };

        for session in stale {
            if let Err(e) = session.close_session_and_delete_subscriptions() {
                log::warn!("[{name}] previous session not released ({e}); server will time it out");
            }
        }
    }

    /// Returns cached tags, or scans and caches them.
    fn load_or_scan(&self, session: Arc<dyn PlcSession>) -> Result<TagSet> {
        #[cfg(feature = "json-cache")]
        if let Some(cache) = &self.cache {
            if cache.exists() && !self.force_rescan {
                match cache.load() {
                    Ok(tags) => {
                        log::info!("[{}] loaded {} tags from cache", self.name, tags.len());
                        return Ok(TagSet::new(tags));
                    }
                    Err(e) => log::warn!("[{}] cache unusable ({e}), rescanning", self.name),
                }
            }
        }

        log::info!("[{}] scanning address space", self.name);

        let browser = OpcUaNodeBrowser::new(session);
        let scanner = TreeScanner::new(&browser, self.filter.as_ref(), self.scan_options.clone());
        let report = scanner.scan(NodeId::objects_folder_id())?;

        // A partial scan is normal on a large server, but a silent one is
        // indistinguishable from a healthy scan. Say so.
        if !report.is_complete() {
            log::warn!(
                "[{}] {} subtrees were unreadable",
                self.name,
                report.skipped.len()
            );
            for (node_id, err) in &report.skipped {
                log::debug!("[{}]   {node_id}: {err}", self.name);
            }
        }

        log::info!("[{}] discovered {} tags", self.name, report.tags.len());

        let tags = report.into_tags();

        #[cfg(feature = "json-cache")]
        if let Some(cache) = &self.cache {
            cache.save(&tags)?;
        }

        Ok(TagSet::new(tags))
    }

    /// Runs the poller on its own thread and its own session.
    fn spawn_poller(&self, tags: Vec<PlcTag>, sink: Arc<dyn TagSink>) {
        if tags.is_empty() {
            return;
        }

        let name = format!("{}-poll", self.name);
        let endpoint_url = self.endpoint_url.clone();
        let connect_options = self.connect_options.clone();
        let monitor_options = self.monitor_options.clone();
        let sessions = self.sessions.clone();
        let stop = self.stop.clone();
        let delay = self.monitor_options.reconnect_delay;

        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let raw = match connect(&endpoint_url, &connect_options) {
                    Ok(raw) => raw,
                    Err(e) => {
                        log::error!("[{name}] could not open polling session: {e}");
                        thread::sleep(delay);
                        continue;
                    }
                };

                let session: Arc<dyn PlcSession> = Arc::new(OpcUaSession::new(raw));
                registry_lock(&sessions).push((name.clone(), session.clone()));

                // Scoped to this session so the watchdog dies with it.
                let session_stop = Arc::new(AtomicBool::new(false));
                spawn_watchdog(
                    name.clone(),
                    endpoint_url.clone(),
                    session.clone(),
                    sink.clone(),
                    session_stop.clone(),
                    monitor_options.health_check_interval,
                );

                log::info!(
                    "[{name}] polling {} tags every {}ms",
                    tags.len(),
                    monitor_options.poll_interval.as_millis()
                );

                Poller::new(session, monitor_options.clone(), name.clone(), sink.clone())
                    .poll_forever(&tags, &stop);

                session_stop.store(true, Ordering::Relaxed);

                if !stop.load(Ordering::Relaxed) {
                    log::warn!("[{name}] reconnecting in {}s", delay.as_secs());
                    thread::sleep(delay);
                }
            }
        });
    }
}

/// Stops a running [`Collector`] and closes its sessions.
///
/// The crate never calls `process::exit`; wire this into your own signal
/// handler, or use [`Collector::handle_ctrl_c`].
#[derive(Clone)]
pub struct CollectorHandle {
    stop: Arc<AtomicBool>,
    sessions: Registry,
}

impl CollectorHandle {
    /// Signals shutdown and closes every session this collector opened.
    ///
    /// Closing matters: a server does not learn an abandoned session is gone
    /// until it times out, and until then its monitored items still count
    /// against the budget. Restart a collector a few times without this and you
    /// exhaust a server that was nowhere near its limit.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);

        for (name, session) in registry_lock(&self.sessions).iter() {
            match session.close_session_and_delete_subscriptions() {
                Ok(()) => log::info!("[{name}] session closed"),
                Err(e) => log::warn!("[{name}] could not close session: {e}"),
            }
        }
    }

    /// Whether shutdown has been signalled.
    pub fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

fn registry_lock(
    registry: &Registry,
) -> std::sync::MutexGuard<'_, Vec<(String, Arc<dyn PlcSession>)>> {
    registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Signals background threads to stop when the pipeline unwinds.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}
