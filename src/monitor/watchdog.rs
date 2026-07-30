//! Reporting connection state transitions.

use crate::session::PlcSession;
use crate::sink::{ConnectionChange, TagSink};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};

/// Watches a session and reports every transition, not every check.
pub(crate) fn spawn_watchdog(
    collector: String,
    endpoint_url: String,
    session: Arc<dyn PlcSession>,
    sink: Arc<dyn TagSink>,
    stop: Arc<AtomicBool>,
    interval: Duration,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut was_connected = session.is_connected();

        while !stop.load(Ordering::Relaxed) {
            thread::sleep(interval);

            if stop.load(Ordering::Relaxed) {
                break;
            }

            let is_connected = session.is_connected();
            if is_connected == was_connected {
                continue;
            }
            was_connected = is_connected;

            sink.connection_changed(&ConnectionChange {
                timestamp: SystemTime::now(),
                collector: &collector,
                endpoint_url: &endpoint_url,
                connected: is_connected,
            });
        }
    })
}
