//! The flat tag record produced by a scan.

use serde::{Deserialize, Serialize};

/// A single variable node discovered on an OPC UA server.
///
/// This is the crate's output type: a scan turns a hierarchical address space
/// into a flat `Vec<PlcTag>` that can be cached, filtered, and fed to a
/// subscription or polling layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PlcTag {
    /// Human-readable name taken from the node's `DisplayName` attribute.
    pub display_name: String,

    /// Node identifier in OPC UA string form, for example `ns=3;s="DB1"."Speed"`.
    ///
    /// Parse it back with `NodeId::from_str` before using it in a service call.
    pub node_id: String,

    /// Node class rendered as text, for example `"Variable"`.
    pub node_class: String,

    /// Slash-joined browse path from the scan root, for example `Machine/Axis1/Speed`.
    ///
    /// Useful for grouping and for building human-facing tag trees, since the
    /// flat list otherwise discards all hierarchy.
    pub path: String,
}

impl PlcTag {
    /// Builds a tag record.
    pub fn new(
        display_name: impl Into<String>,
        node_id: impl Into<String>,
        node_class: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self {
            display_name: display_name.into(),
            node_id: node_id.into(),
            node_class: node_class.into(),
            path: path.into(),
        }
    }
}