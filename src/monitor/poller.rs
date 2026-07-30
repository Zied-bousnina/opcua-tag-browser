//! Cyclic reading for tags the server would not monitor.

use super::MonitorOptions;
use crate::session::PlcSession;
use crate::sink::{Source, TagChange, TagSink};
use crate::tag::PlcTag;
use crate::variant::{format_quality, format_variant, variant_as_f64};
use opcua::client::prelude::*;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::SystemTime;

/// Floor for adaptive batch sizing, so a failing batch cannot stall the loop.
const MIN_BATCH_SIZE: usize = 10;

/// What was last seen for a tag, for change detection.
struct LastSeen {
    rendered: String,
    quality: String,
    numeric: Option<f64>,
}

/// Reads tags in batches and reports those whose value or quality changed.
pub(crate) struct Poller {
    session: Arc<dyn PlcSession>,
    options: MonitorOptions,
    collector: String,
    sink: Arc<dyn TagSink>,
}

impl Poller {
    pub(crate) fn new(
        session: Arc<dyn PlcSession>,
        options: MonitorOptions,
        collector: String,
        sink: Arc<dyn TagSink>,
    ) -> Self {
        Self {
            session,
            options,
            collector,
            sink,
        }
    }

    /// Polls until `stop` is set or the connection drops.
    pub(crate) fn poll_forever(&self, tags: &[PlcTag], stop: &Arc<AtomicBool>) {
        // Parse node IDs once, not on every pass.
        let readable: Vec<(&PlcTag, ReadValueId)> = tags
            .iter()
            .filter_map(|tag| {
                let node_id = NodeId::from_str(&tag.node_id).ok()?;
                Some((tag, ReadValueId::from(node_id)))
            })
            .collect();

        if readable.is_empty() {
            return;
        }

        let mut batch_size = self.options.poll_batch_size;
        if let Some(limit) = self.server_max_nodes_per_read() {
            if limit < batch_size {
                log::info!("[{}] server caps reads at {limit} nodes", self.collector);
                batch_size = limit;
            }
        }

        // Letting the server serve cached values up to one cycle old avoids
        // forcing a fresh device read for every tag on every pass.
        let max_age = self.options.poll_interval.as_millis() as f64;

        let mut last: HashMap<String, LastSeen> = HashMap::new();

        while !stop.load(Ordering::Relaxed) {
            let mut start = 0;

            while start < readable.len() && !stop.load(Ordering::Relaxed) {
                let end = (start + batch_size).min(readable.len());
                let batch = &readable[start..end];
                let requests: Vec<ReadValueId> = batch.iter().map(|(_, rv)| rv.clone()).collect();

                match self
                    .session
                    .read(&requests, TimestampsToReturn::Both, max_age)
                {
                    Ok(values) => {
                        self.report_changes(batch, &values, &mut last);
                        start = end;
                    }
                    Err(status) => {
                        if !self.session.is_connected() {
                            log::warn!("[{}] connection lost, ending poll loop", self.collector);
                            return;
                        }

                        if batch_size > MIN_BATCH_SIZE {
                            // Most read failures at scale are size related.
                            // Halve and retry the same range before giving up.
                            batch_size = (batch_size / 2).max(MIN_BATCH_SIZE);
                            log::warn!(
                                "[{}] read of {} failed ({status}), batch size now {batch_size}",
                                self.collector,
                                batch.len()
                            );
                        } else {
                            log::debug!(
                                "[{}] skipping batch of {} at minimum size: {status}",
                                self.collector,
                                batch.len()
                            );
                            start = end;
                        }
                    }
                }
            }

            thread::sleep(self.options.poll_interval);
        }
    }

    /// Reports any tag in the batch whose value or quality changed.
    fn report_changes(
        &self,
        batch: &[(&PlcTag, ReadValueId)],
        values: &[DataValue],
        last: &mut HashMap<String, LastSeen>,
    ) {
        for ((tag, _), data_value) in batch.iter().zip(values.iter()) {
            let Some(ref variant) = data_value.value else {
                continue;
            };

            // Members of structured tags are read individually.
            if matches!(variant, Variant::ExtensionObject(_)) {
                continue;
            }

            let rendered = format_variant(variant);
            let quality = format_quality(data_value.status);
            let numeric = variant_as_f64(variant);

            if !self.is_change(last.get(&tag.node_id), &rendered, &quality, numeric) {
                continue;
            }

            let good = data_value.status.map(|s| s.is_good()).unwrap_or(true);

            self.sink.tag_changed(&TagChange {
                timestamp: SystemTime::now(),
                collector: &self.collector,
                tag,
                variant,
                value: &rendered,
                quality: &quality,
                good,
                source: Source::Poll,
            });

            last.insert(
                tag.node_id.clone(),
                LastSeen {
                    rendered,
                    quality,
                    numeric,
                },
            );
        }
    }

    /// Whether a reading differs enough from the previous one to report.
    fn is_change(
        &self,
        previous: Option<&LastSeen>,
        rendered: &str,
        quality: &str,
        numeric: Option<f64>,
    ) -> bool {
        let Some(previous) = previous else {
            return true; // First reading is always reported.
        };

        // A Good to Bad transition matters even at a constant value.
        if previous.quality != quality {
            return true;
        }

        // Analogue signals jitter; suppress movement below the deadband.
        if let (Some(threshold), Some(new), Some(old)) =
            (self.options.deadband, numeric, previous.numeric)
        {
            return (new - old).abs() >= threshold;
        }

        previous.rendered != rendered
    }

    /// Asks the server for its own `MaxNodesPerRead` limit.
    fn server_max_nodes_per_read(&self) -> Option<usize> {
        let node_id: NodeId =
            VariableId::Server_ServerCapabilities_OperationLimits_MaxNodesPerRead.into();

        let values = self
            .session
            .read(
                &[ReadValueId::from(node_id)],
                TimestampsToReturn::Neither,
                0.0,
            )
            .ok()?;

        match values.into_iter().next()?.value? {
            Variant::UInt32(limit) if limit > 0 => Some(limit as usize),
            _ => None,
        }
    }
}
