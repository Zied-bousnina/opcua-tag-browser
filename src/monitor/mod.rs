//! Live value collection: subscriptions, polling, and connection watching.
//!
//! Available with the `monitoring` feature, on by default.

mod poller;
mod subscriber;
mod watchdog;

pub(crate) use poller::Poller;
pub(crate) use subscriber::Subscriber;
pub(crate) use watchdog::spawn_watchdog;

use std::time::Duration;

/// Subscription and polling tuning.
///
/// The defaults come from a Siemens PLC exposing about 2,500 tags. They are a
/// reasonable starting point, not a specification — measure your own server.
///
/// ```
/// use opcua_tag_browser::MonitorOptions;
/// use std::time::Duration;
///
/// let options = MonitorOptions::default()
///     .max_subscribed_items(1200)
///     .poll_interval(Duration::from_millis(500));
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MonitorOptions {
    /// Ceiling on monitored items. Everything above it is polled instead.
    ///
    /// Servers advertise a limit they will not honour. Raise this until you see
    /// `BadTooManyMonitoredItems`, then back off with margin. Getting it wrong
    /// costs latency, not data: the overflow is polled.
    pub max_subscribed_items: usize,
    /// Tags grouped into one subscription.
    pub sub_chunk_size: usize,
    /// Items per `CreateMonitoredItems` request.
    pub request_batch_size: usize,
    /// How often the server samples each tag.
    pub sampling_interval: Duration,
    /// How often the server pushes accumulated changes.
    pub publishing_interval: Duration,
    /// Publish cycles an idle subscription survives.
    pub lifetime_count: u32,
    /// Publish cycles before a keep-alive is sent.
    pub max_keep_alive_count: u32,
    /// Pause between polling passes.
    pub poll_interval: Duration,
    /// Nodes per `Read` request, before adaptive reduction.
    pub poll_batch_size: usize,
    /// How often connectivity is sampled.
    ///
    /// A cheap local check (no network call), so this can be short without
    /// meaningfully adding load; a short interval means a dropped connection
    /// is noticed, and reported to `TagSink::connection_changed`, sooner.
    pub health_check_interval: Duration,
    /// Ceiling on the poller's reconnect backoff.
    ///
    /// The poller retries with exponential backoff starting at
    /// [`reconnect_backoff_min`](Self::reconnect_backoff_min), doubling on
    /// every failed attempt, capped here. Kept as `reconnect_delay` rather
    /// than renamed, since it is still the number that matters most: how long
    /// the poller waits between attempts once an outage is clearly not a
    /// short blip.
    pub reconnect_delay: Duration,
    /// Starting point for the poller's reconnect backoff.
    ///
    /// Short by default so a brief network blip recovers almost immediately;
    /// the delay only grows if reconnecting keeps failing, and resets the
    /// moment a connection succeeds.
    pub reconnect_backoff_min: Duration,
    /// Ignore numeric changes smaller than this, when set.
    ///
    /// Analogue signals jitter. A deadband of `0.5` suppresses noise below half
    /// a unit while still reporting real movement.
    pub deadband: Option<f64>,
    /// Starting point for [`Collector`](crate::Collector)'s own restart
    /// backoff, used when a run attempt ends for a reason other than
    /// `CollectorHandle::stop` (see `Collector::resilient`).
    pub restart_backoff_min: Duration,
    /// Ceiling on `Collector`'s restart backoff.
    pub restart_backoff_max: Duration,
}

impl Default for MonitorOptions {
    fn default() -> Self {
        Self {
            max_subscribed_items: 800,
            sub_chunk_size: 500,
            request_batch_size: 50,
            sampling_interval: Duration::from_millis(250),
            publishing_interval: Duration::from_millis(100),
            lifetime_count: 100,
            max_keep_alive_count: 10,
            poll_interval: Duration::from_millis(1000),
            poll_batch_size: 50,
            health_check_interval: Duration::from_secs(1),
            reconnect_delay: Duration::from_secs(10),
            reconnect_backoff_min: Duration::from_millis(500),
            deadband: None,
            restart_backoff_min: Duration::from_secs(1),
            restart_backoff_max: Duration::from_secs(30),
        }
    }
}

impl MonitorOptions {
    /// Sets the monitored-item ceiling.
    pub fn max_subscribed_items(mut self, n: usize) -> Self {
        self.max_subscribed_items = n;
        self
    }

    /// Sets how often the server samples each subscribed tag.
    pub fn sampling_interval(mut self, d: Duration) -> Self {
        self.sampling_interval = d;
        self
    }

    /// Sets how often the server pushes accumulated changes.
    pub fn publishing_interval(mut self, d: Duration) -> Self {
        self.publishing_interval = d;
        self
    }

    /// Sets the pause between polling passes.
    pub fn poll_interval(mut self, d: Duration) -> Self {
        self.poll_interval = d;
        self
    }

    /// Sets the ceiling on the poller's reconnect backoff.
    pub fn reconnect_delay(mut self, d: Duration) -> Self {
        self.reconnect_delay = d;
        self
    }

    /// Sets the starting point for the poller's reconnect backoff.
    pub fn reconnect_backoff_min(mut self, d: Duration) -> Self {
        self.reconnect_backoff_min = d;
        self
    }

    /// Sets how often connectivity is sampled.
    pub fn health_check_interval(mut self, d: Duration) -> Self {
        self.health_check_interval = d;
        self
    }

    /// Suppresses numeric changes smaller than `threshold`.
    pub fn deadband(mut self, threshold: f64) -> Self {
        self.deadband = Some(threshold);
        self
    }

    /// Sets the starting point for `Collector`'s own restart backoff.
    pub fn restart_backoff_min(mut self, d: Duration) -> Self {
        self.restart_backoff_min = d;
        self
    }

    /// Sets the ceiling on `Collector`'s restart backoff.
    pub fn restart_backoff_max(mut self, d: Duration) -> Self {
        self.restart_backoff_max = d;
        self
    }
}
