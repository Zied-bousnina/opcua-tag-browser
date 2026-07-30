//! A sink that writes through the `log` facade.

use crate::sink::{ConnectionChange, TagChange, TagSink};

/// Logs every change at `info`.
///
/// Useful for getting a collector running before deciding where data should
/// actually go.
///
/// ```no_run
/// use opcua_tag_browser::{sinks::LogSink, Collector};
///
/// # fn main() -> opcua_tag_browser::Result<()> {
/// Collector::new("line1", "opc.tcp://localhost:4840")
///     .insecure()
///     .sink(LogSink)
///     .run()
/// # }
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct LogSink;

impl TagSink for LogSink {
    fn tag_changed(&self, event: &TagChange<'_>) {
        log::info!(
            "[{}] {} = {} ({}, {})",
            event.collector,
            event.path(),
            event.value,
            event.quality,
            event.source.as_str()
        );
    }

    fn connection_changed(&self, event: &ConnectionChange<'_>) {
        if event.connected {
            log::info!("[{}] connection restored", event.collector);
        } else {
            log::warn!("[{}] connection lost", event.collector);
        }
    }
}
