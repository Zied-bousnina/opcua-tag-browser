//! A sink that writes daily JSONL files.

use crate::sink::{ConnectionChange, TagChange, TagSink};
use chrono::{DateTime, Local, SecondsFormat};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::SystemTime;

/// Writes one JSON object per line to `<dir>/<collector>_<YYYYMMDD>.jsonl`.
///
/// Serialization and file I/O happen on a dedicated thread, so the subscription
/// callback never blocks on disk. Files rotate at local midnight and are
/// flushed after every line, so `tail -f` shows live data.
///
/// ```no_run
/// use opcua_tag_browser::Collector;
///
/// # fn main() -> opcua_tag_browser::Result<()> {
/// Collector::new("line1", "opc.tcp://localhost:4840")
///     .insecure()
///     .jsonl("logs")
///     .run()
/// # }
/// ```
pub struct JsonlSink {
    tx: Mutex<Sender<Line>>,
}

impl JsonlSink {
    /// Creates the directory if needed and starts the writer thread.
    pub fn new(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;

        let (tx, rx) = mpsc::channel::<Line>();

        thread::spawn(move || {
            // One open handle per (collector, date) so files rotate cleanly.
            let mut writers: HashMap<(String, String), BufWriter<File>> = HashMap::new();

            for line in rx {
                let (collector, at) = match &line {
                    Line::Tag(t) => (t.collector.clone(), t.at),
                    Line::Connection(c) => (c.collector.clone(), c.at),
                };

                let key = date_key(at);
                let writer = match writer_for(&mut writers, &dir, &collector, &key) {
                    Ok(w) => w,
                    Err(e) => {
                        log::error!("could not open log file for {collector}: {e}");
                        continue;
                    }
                };

                let json = match &line {
                    Line::Tag(t) => serde_json::to_string(t),
                    Line::Connection(c) => serde_json::to_string(c),
                };

                match json {
                    Ok(json) => {
                        if writeln!(writer, "{json}").is_ok() {
                            let _ = writer.flush();
                        }
                    }
                    Err(e) => log::error!("could not serialize event: {e}"),
                }
            }
        });

        Ok(Self { tx: Mutex::new(tx) })
    }

    fn send(&self, line: Line) {
        let tx = self
            .tx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = tx.send(line);
    }
}

impl TagSink for JsonlSink {
    fn tag_changed(&self, event: &TagChange<'_>) {
        self.send(Line::Tag(TagLine {
            at: event.timestamp,
            timestamp: format_timestamp(event.timestamp),
            collector: event.collector.to_string(),
            tag_id: event.tag.node_id.clone(),
            display_name: event.tag.display_name.clone(),
            path: event.tag.path.clone(),
            value: event.value.to_string(),
            quality: event.quality.to_string(),
            source: event.source.as_str(),
        }));
    }

    fn connection_changed(&self, event: &ConnectionChange<'_>) {
        self.send(Line::Connection(ConnectionLine {
            at: event.timestamp,
            timestamp: format_timestamp(event.timestamp),
            collector: event.collector.to_string(),
            endpoint_url: event.endpoint_url.to_string(),
            status: if event.connected { "restored" } else { "lost" },
        }));
    }
}

enum Line {
    Tag(TagLine),
    Connection(ConnectionLine),
}

#[derive(Serialize)]
struct TagLine {
    #[serde(skip)]
    at: SystemTime,
    timestamp: String,
    collector: String,
    tag_id: String,
    display_name: String,
    path: String,
    value: String,
    quality: String,
    source: &'static str,
}

#[derive(Serialize)]
struct ConnectionLine {
    #[serde(skip)]
    at: SystemTime,
    timestamp: String,
    collector: String,
    endpoint_url: String,
    status: &'static str,
}

fn writer_for<'a>(
    writers: &'a mut HashMap<(String, String), BufWriter<File>>,
    dir: &Path,
    collector: &str,
    date_key: &str,
) -> std::io::Result<&'a mut BufWriter<File>> {
    let key = (collector.to_string(), date_key.to_string());

    if !writers.contains_key(&key) {
        let path = dir.join(format!("{collector}_{date_key}.jsonl"));
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        writers.insert(key.clone(), BufWriter::new(file));
    }

    Ok(writers.get_mut(&key).expect("just inserted"))
}

fn date_key(t: SystemTime) -> String {
    let local: DateTime<Local> = t.into();
    local.format("%Y%m%d").to_string()
}

fn format_timestamp(t: SystemTime) -> String {
    let local: DateTime<Local> = t.into();
    local.to_rfc3339_opts(SecondsFormat::Millis, false)
}
