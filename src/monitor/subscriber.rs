//! Server-push monitoring, chunked to what the server will accept.

use super::MonitorOptions;
use crate::sink::{Source, TagChange, TagSink};
use crate::tag::PlcTag;
use crate::variant::{format_quality, format_variant};
use opcua::client::prelude::*;
use opcua::sync::RwLock;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::SystemTime;

/// Subscribes tags in chunks and forwards every change the server pushes.
pub(crate) struct Subscriber {
    raw: Arc<RwLock<Session>>,
    options: MonitorOptions,
    collector: String,
    sink: Arc<dyn TagSink>,
}

impl Subscriber {
    pub(crate) fn new(
        raw: Arc<RwLock<Session>>,
        options: MonitorOptions,
        collector: String,
        sink: Arc<dyn TagSink>,
    ) -> Self {
        Self {
            raw,
            options,
            collector,
            sink,
        }
    }

    /// Subscribes to `tags`, returning those the server refused.
    ///
    /// Rejected tags are not an error: the caller polls them instead.
    pub(crate) fn subscribe_all(&self, tags: &[PlcTag]) -> Vec<PlcTag> {
        // queue_size 1 with discard_oldest keeps only the latest value per tag,
        // which is what a change log wants.
        let params = MonitoringParameters {
            sampling_interval: self.options.sampling_interval.as_millis() as f64,
            queue_size: 1,
            discard_oldest: true,
            ..Default::default()
        };

        let requests: Vec<(&PlcTag, MonitoredItemCreateRequest)> = tags
            .iter()
            .filter_map(|tag| {
                let node_id = NodeId::from_str(&tag.node_id).ok()?;
                Some((
                    tag,
                    MonitoredItemCreateRequest::new(
                        ReadValueId::from(node_id),
                        MonitoringMode::Reporting,
                        params.clone(),
                    ),
                ))
            })
            .collect();

        // The callback receives node IDs only, so carry full tags across.
        let by_node_id: Arc<HashMap<String, PlcTag>> = Arc::new(
            tags.iter()
                .map(|t| (t.node_id.clone(), t.clone()))
                .collect(),
        );

        let mut rejected = Vec::new();

        for (index, chunk) in requests.chunks(self.options.sub_chunk_size).enumerate() {
            let sub_id = match self.create_subscription(by_node_id.clone()) {
                Ok(id) => id,
                Err(status) => {
                    log::warn!(
                        "[{}] could not create subscription {}: {status}",
                        self.collector,
                        index + 1
                    );
                    rejected.extend(chunk.iter().map(|(t, _)| (*t).clone()));
                    continue;
                }
            };

            log::info!(
                "[{}] subscription {} covers {} items",
                self.collector,
                index + 1,
                chunk.len()
            );

            for batch in chunk.chunks(self.options.request_batch_size) {
                let (batch_tags, batch_requests): (Vec<&PlcTag>, Vec<_>) =
                    batch.iter().map(|(t, r)| (*t, r.clone())).unzip();

                match self.raw.write().create_monitored_items(
                    sub_id,
                    TimestampsToReturn::Both,
                    &batch_requests,
                ) {
                    Ok(results) => {
                        // Per-item status matters. Ignoring it is how a server
                        // silently drops half your tags without any error.
                        for (tag, result) in batch_tags.iter().zip(results.iter()) {
                            if result.status_code.is_bad() {
                                log::debug!(
                                    "[{}] rejected {}: {}",
                                    self.collector,
                                    tag.node_id,
                                    result.status_code
                                );
                                rejected.push((*tag).clone());
                            }
                        }
                    }
                    Err(status) => {
                        log::warn!(
                            "[{}] batch of {} rejected: {status}",
                            self.collector,
                            batch_tags.len()
                        );
                        rejected.extend(batch_tags.into_iter().cloned());
                    }
                }
            }
        }

        if !rejected.is_empty() {
            log::info!(
                "[{}] {} tags refused, falling back to polling",
                self.collector,
                rejected.len()
            );
        }

        rejected
    }

    /// Creates one subscription whose callback forwards changes to the sink.
    fn create_subscription(
        &self,
        by_node_id: Arc<HashMap<String, PlcTag>>,
    ) -> Result<u32, StatusCode> {
        let collector = self.collector.clone();
        let sink = self.sink.clone();

        self.raw.write().create_subscription(
            self.options.publishing_interval.as_millis() as f64,
            self.options.lifetime_count,
            self.options.max_keep_alive_count,
            0,
            0,
            true,
            DataChangeCallback::new(move |items| {
                for item in items {
                    let node_id = item.item_to_monitor().node_id.to_string();

                    let Some(latest) = item.values().last() else {
                        continue;
                    };
                    let Some(ref variant) = latest.value else {
                        continue;
                    };

                    // Struct members are subscribed individually, so the
                    // container itself carries nothing useful.
                    if matches!(variant, Variant::ExtensionObject(_)) {
                        continue;
                    }

                    let Some(tag) = by_node_id.get(&node_id) else {
                        continue;
                    };

                    let good = latest.status.map(|s| s.is_good()).unwrap_or(true);

                    sink.tag_changed(&TagChange {
                        timestamp: SystemTime::now(),
                        collector: &collector,
                        tag,
                        variant,
                        value: &format_variant(variant),
                        quality: &format_quality(latest.status),
                        good,
                        source: Source::Subscribe,
                    });
                }
            }),
        )
    }
}
