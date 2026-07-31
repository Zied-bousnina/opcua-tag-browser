//! The flat tag record produced by a scan.

use serde::{Deserialize, Serialize};

/// A single variable node discovered on an OPC UA server.
///
/// This is the crate's output type: a scan turns a hierarchical address space
/// into a flat `Vec<PlcTag>` that can be cached, filtered, and fed to a
/// subscription or polling layer.
///
/// The attribute fields (`data_type` through `historizing`) come from the
/// `Variable` attributes in OPC 10000-3 Table 13. They are `None` unless the
/// scan read them (see [`Collector::read_attributes`](crate::Collector::read_attributes)),
/// so a cache written before this field existed still deserializes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PlcTag {
    /// Human-readable name taken from the node's `DisplayName` attribute.
    pub display_name: String,

    /// The node's `BrowseName`, namespace-qualified.
    ///
    /// Unlike `display_name`, this is stable across server locales, which is
    /// why [`path`](Self::path) is built from it rather than `display_name`.
    pub browse_name: String,

    /// Node identifier in OPC UA string form, for example `ns=3;s="DB1"."Speed"`.
    ///
    /// Parse it back with `NodeId::from_str` before using it in a service call.
    pub node_id: String,

    /// Node class rendered as text, for example `"Variable"`.
    pub node_class: String,

    /// Slash-joined `BrowseName`s from the scan root, for example
    /// `Machine/Axis1/Speed`.
    ///
    /// Per OPC 10000-3 section 6.2.5 a browse path is built from `BrowseName`, not
    /// `DisplayName`: `DisplayName` is localized and not required to be
    /// unique among siblings, so building paths from it would break under a
    /// locale change.
    pub path: String,

    /// `NodeId` of the value's `DataType` attribute, in string form.
    ///
    /// `None` if attributes were not read.
    pub data_type: Option<String>,

    /// `ValueRank` attribute: `-1` for a scalar, `0` for an array of unknown
    /// dimensions, `n >= 1` for an array with that many dimensions.
    ///
    /// `None` if attributes were not read.
    pub value_rank: Option<i32>,

    /// `AccessLevel` attribute bits (OPC 10000-3 §8.57).
    ///
    /// `None` if attributes were not read.
    pub access_level: Option<u8>,

    /// Server's own `MinimumSamplingInterval` attribute, in milliseconds.
    ///
    /// Per Table 13, `0` means the server samples continuously and a negative
    /// value means indeterminate. `None` if attributes were not read.
    pub min_sampling_interval: Option<f64>,

    /// `Historizing` attribute: whether the server is actively collecting
    /// history for this value.
    ///
    /// `None` if attributes were not read.
    pub historizing: Option<bool>,

    /// `TypeDefinition` node ID reached while browsing, for example
    /// distinguishing `PropertyType` from `BaseDataVariableType`.
    pub type_definition: Option<String>,

    /// Reference type that reached this node while browsing, for example
    /// `HasProperty` versus `HasComponent`.
    pub reference_type: Option<String>,
}

/// `AccessLevel` bit for "current value is readable" (OPC 10000-3 §8.57).
const ACCESS_LEVEL_CURRENT_READ: u8 = 0x01;
/// `AccessLevel` bit for "current value is writable".
const ACCESS_LEVEL_CURRENT_WRITE: u8 = 0x02;
/// `AccessLevel` bit for "history is readable".
const ACCESS_LEVEL_HISTORY_READ: u8 = 0x04;

/// `HasProperty` reference type numeric identifier (OPC 10000-5).
const HAS_PROPERTY_IDENTIFIER: &str = "46";

impl PlcTag {
    /// Builds a tag record with no attributes read.
    pub fn new(
        display_name: impl Into<String>,
        browse_name: impl Into<String>,
        node_id: impl Into<String>,
        node_class: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self {
            display_name: display_name.into(),
            browse_name: browse_name.into(),
            node_id: node_id.into(),
            node_class: node_class.into(),
            path: path.into(),
            data_type: None,
            value_rank: None,
            access_level: None,
            min_sampling_interval: None,
            historizing: None,
            type_definition: None,
            reference_type: None,
        }
    }

    /// Whether the server's `AccessLevel` says this value is readable.
    ///
    /// Assumes readable when `AccessLevel` was not read, since that is the
    /// overwhelmingly common case and refusing to read would be surprising.
    pub fn is_readable(&self) -> bool {
        self.access_level
            .map_or(true, |a| a & ACCESS_LEVEL_CURRENT_READ != 0)
    }

    /// Whether the server's `AccessLevel` says this value is writable.
    ///
    /// Assumes writable when `AccessLevel` was not read, so this only ever
    /// blocks a write the server would already have rejected — it does not
    /// change behavior for tags scanned before this field existed, or
    /// scanned with attribute reading turned off.
    pub fn is_writable(&self) -> bool {
        self.access_level
            .map_or(true, |a| a & ACCESS_LEVEL_CURRENT_WRITE != 0)
    }

    /// Whether the server's `AccessLevel` says history is readable.
    pub fn has_history(&self) -> bool {
        self.access_level
            .is_some_and(|a| a & ACCESS_LEVEL_HISTORY_READ != 0)
    }

    /// Whether this is an array, per `ValueRank`.
    pub fn is_array(&self) -> bool {
        self.value_rank.is_some_and(|r| r >= 0)
    }

    /// Whether this node was reached via `HasProperty` rather than
    /// `HasComponent` or another hierarchical reference.
    ///
    /// Properties are metadata (`EngineeringUnits`, `EURange`) rather than
    /// process data.
    pub fn is_property(&self) -> bool {
        self.reference_type
            .as_deref()
            .is_some_and(|r| r.contains(HAS_PROPERTY_IDENTIFIER))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag_with_access_level(access_level: Option<u8>) -> PlcTag {
        let mut tag = PlcTag::new("Speed", "Speed", "ns=2;s=speed", "Variable", "Speed");
        tag.access_level = access_level;
        tag
    }

    #[test]
    fn unread_access_level_assumes_readable_and_writable() {
        let tag = tag_with_access_level(None);
        assert!(tag.is_readable());
        assert!(tag.is_writable());
        assert!(!tag.has_history());
    }

    #[test]
    fn access_level_bits_are_interpreted() {
        let read_only = tag_with_access_level(Some(0x01));
        assert!(read_only.is_readable());
        assert!(!read_only.is_writable());

        let read_write = tag_with_access_level(Some(0x01 | 0x02));
        assert!(read_write.is_readable());
        assert!(read_write.is_writable());

        let with_history = tag_with_access_level(Some(0x04));
        assert!(with_history.has_history());
    }

    #[test]
    fn value_rank_determines_is_array() {
        let mut scalar = PlcTag::new("S", "S", "ns=2;s=s", "Variable", "S");
        scalar.value_rank = Some(-1);
        assert!(!scalar.is_array());

        let mut array = PlcTag::new("A", "A", "ns=2;s=a", "Variable", "A");
        array.value_rank = Some(1);
        assert!(array.is_array());
    }

    #[test]
    fn reference_type_determines_is_property() {
        let mut plain = PlcTag::new("S", "S", "ns=2;s=s", "Variable", "S");
        assert!(!plain.is_property());

        // i=46 is HasProperty (OPC 10000-5).
        plain.reference_type = Some("i=46".to_string());
        assert!(plain.is_property());
    }
}
