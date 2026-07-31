//! Reading the `Variable` attributes from OPC 10000-3 Table 13.
//!
//! A scan's `Browse` pass only returns `BrowseName`, `DisplayName`,
//! `NodeClass`, `TypeDefinition`, and the reference type. `DataType`,
//! `ValueRank`, `AccessLevel`, `MinimumSamplingInterval`, and `Historizing`
//! need a separate `Read`, since `Browse` does not carry them.

use crate::session::PlcSession;
use crate::tag::PlcTag;
use crate::variant::{variant_as_bool, variant_as_f64, variant_as_i64};
use opcua::client::prelude::*;
use std::str::FromStr;

/// Nodes read per `Read` service call, before chunking for the next batch.
///
/// Each node contributes five [`ReadValueId`]s, so this keeps a single
/// request comfortably under a typical server's `MaxNodesPerRead`.
const NODES_PER_BATCH: usize = 200;

/// Reads the Table 13 attributes for every `Variable` tag and fills them in.
///
/// Tags whose node ID does not parse, or whose read comes back bad, are left
/// with their attribute fields `None` rather than failing the whole scan: a
/// handful of unreadable nodes should not block discovery of the rest.
pub(crate) fn fill_variable_attributes(session: &dyn PlcSession, tags: &mut [PlcTag]) {
    let variable_indices: Vec<usize> = tags
        .iter()
        .enumerate()
        .filter(|(_, t)| t.node_class == "Variable")
        .map(|(i, _)| i)
        .collect();

    for chunk in variable_indices.chunks(NODES_PER_BATCH) {
        let mut requests = Vec::with_capacity(chunk.len() * 5);
        let mut parsed = Vec::with_capacity(chunk.len());

        for &index in chunk {
            let Ok(node_id) = NodeId::from_str(&tags[index].node_id) else {
                continue;
            };
            for attribute in [
                AttributeId::DataType,
                AttributeId::ValueRank,
                AttributeId::AccessLevel,
                AttributeId::MinimumSamplingInterval,
                AttributeId::Historizing,
            ] {
                requests.push(ReadValueId {
                    node_id: node_id.clone(),
                    attribute_id: attribute as u32,
                    index_range: UAString::null(),
                    data_encoding: QualifiedName::null(),
                });
            }
            parsed.push(index);
        }

        if requests.is_empty() {
            continue;
        }

        let values = match session.read(&requests, TimestampsToReturn::Neither, 0.0) {
            Ok(values) => values,
            Err(status) => {
                log::warn!("attribute read failed for {} nodes: {status}", parsed.len());
                continue;
            }
        };

        for (position, &index) in parsed.iter().enumerate() {
            let base = position * 5;
            let Some(results) = values.get(base..base + 5) else {
                continue;
            };
            apply(&mut tags[index], results);
        }
    }
}

/// Applies one node's five attribute reads, in `[DataType, ValueRank,
/// AccessLevel, MinimumSamplingInterval, Historizing]` order.
fn apply(tag: &mut PlcTag, results: &[DataValue]) {
    tag.data_type = value_of(&results[0]).map(crate::variant::format_variant);
    tag.value_rank = value_of(&results[1]).and_then(variant_as_i64).map(|v| v as i32);
    tag.access_level = value_of(&results[2]).and_then(variant_as_i64).map(|v| v as u8);
    tag.min_sampling_interval = value_of(&results[3]).and_then(variant_as_f64);
    tag.historizing = value_of(&results[4]).and_then(variant_as_bool);
}

/// Extracts the value from a `DataValue`, unless the server reported a bad
/// status for it.
fn value_of(data_value: &DataValue) -> Option<&Variant> {
    if data_value.status.is_some_and(|s| !s.is_good()) {
        return None;
    }
    data_value.value.as_ref()
}
