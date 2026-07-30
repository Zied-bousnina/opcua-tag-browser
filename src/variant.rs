//! Rendering and interpreting OPC UA values.

use opcua::client::prelude::{StatusCode, Variant};

/// Formats an OPC UA [`Variant`] into a compact display string.
///
/// Scalars render as their natural text form (`true`, `42`, `3.5`), arrays as
/// `[a, b, c]`, and byte strings as lowercase hex with an `0x` prefix.
/// Structured values render as a placeholder, since decoding an
/// `ExtensionObject` requires the vendor's type dictionary.
///
/// ```
/// use opcua_tag_browser::format_variant;
/// use opcua_tag_browser::opcua::client::prelude::Variant;
///
/// assert_eq!(format_variant(&Variant::Boolean(true)), "true");
/// assert_eq!(format_variant(&Variant::Int32(42)), "42");
/// ```
pub fn format_variant(variant: &Variant) -> String {
    match variant {
        Variant::Empty => "null".to_string(),
        Variant::Boolean(v) => v.to_string(),
        Variant::SByte(v) => v.to_string(),
        Variant::Byte(v) => v.to_string(),
        Variant::Int16(v) => v.to_string(),
        Variant::UInt16(v) => v.to_string(),
        Variant::Int32(v) => v.to_string(),
        Variant::UInt32(v) => v.to_string(),
        Variant::Int64(v) => v.to_string(),
        Variant::UInt64(v) => v.to_string(),
        Variant::Float(v) => v.to_string(),
        Variant::Double(v) => v.to_string(),
        Variant::String(v) => v.to_string(),
        Variant::DateTime(v) => v.to_string(),
        Variant::Guid(v) => v.to_string(),
        Variant::StatusCode(v) => v.to_string(),
        Variant::NodeId(v) => v.to_string(),
        Variant::ExpandedNodeId(v) => v.to_string(),
        Variant::LocalizedText(v) => v.to_string(),
        Variant::QualifiedName(v) => v.name.to_string(),
        Variant::XmlElement(v) => v.to_string(),
        Variant::ByteString(v) => match &v.value {
            Some(bytes) => {
                let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
                format!("0x{}", hex)
            }
            None => "null".to_string(),
        },
        Variant::ExtensionObject(_) => "<structured value>".to_string(),
        Variant::DataValue(_) => "<data value>".to_string(),
        Variant::DiagnosticInfo(_) => "<diagnostic info>".to_string(),
        Variant::Variant(inner) => format_variant(inner),
        Variant::Array(array) => {
            let items: Vec<String> = array.values.iter().map(format_variant).collect();
            format!("[{}]", items.join(", "))
        }
    }
}

/// Renders the status code attached to a `DataValue` as a short quality label.
///
/// An absent status means Good, per the OPC UA specification, so `None` maps to
/// `"GOOD"` rather than to an unknown state.
pub fn format_quality(status: Option<StatusCode>) -> String {
    match status {
        None => "GOOD".to_string(),
        Some(s) if s.is_good() => "GOOD".to_string(),
        Some(s) if s.is_uncertain() => format!("UNCERTAIN ({})", s),
        Some(s) => format!("BAD ({})", s),
    }
}

/// Interprets a [`Variant`] as a float, if it holds a number.
///
/// ```
/// use opcua_tag_browser::variant_as_f64;
/// use opcua_tag_browser::opcua::client::prelude::Variant;
///
/// assert_eq!(variant_as_f64(&Variant::Int32(42)), Some(42.0));
/// assert_eq!(variant_as_f64(&Variant::Boolean(true)), None);
/// ```
pub fn variant_as_f64(variant: &Variant) -> Option<f64> {
    match variant {
        Variant::SByte(v) => Some(*v as f64),
        Variant::Byte(v) => Some(*v as f64),
        Variant::Int16(v) => Some(*v as f64),
        Variant::UInt16(v) => Some(*v as f64),
        Variant::Int32(v) => Some(*v as f64),
        Variant::UInt32(v) => Some(*v as f64),
        Variant::Int64(v) => Some(*v as f64),
        Variant::UInt64(v) => Some(*v as f64),
        Variant::Float(v) => Some(*v as f64),
        Variant::Double(v) => Some(*v),
        Variant::Variant(inner) => variant_as_f64(inner),
        _ => None,
    }
}

/// Interprets a [`Variant`] as a signed integer, if it holds an integral value.
///
/// Floats are rejected rather than truncated: silently dropping a fractional
/// part is rarely what a caller wanted.
pub fn variant_as_i64(variant: &Variant) -> Option<i64> {
    match variant {
        Variant::SByte(v) => Some(*v as i64),
        Variant::Byte(v) => Some(*v as i64),
        Variant::Int16(v) => Some(*v as i64),
        Variant::UInt16(v) => Some(*v as i64),
        Variant::Int32(v) => Some(*v as i64),
        Variant::UInt32(v) => Some(*v as i64),
        Variant::Int64(v) => Some(*v),
        Variant::UInt64(v) => i64::try_from(*v).ok(),
        Variant::Variant(inner) => variant_as_i64(inner),
        _ => None,
    }
}

/// Interprets a [`Variant`] as a boolean, if it holds one.
pub fn variant_as_bool(variant: &Variant) -> Option<bool> {
    match variant {
        Variant::Boolean(v) => Some(*v),
        Variant::Variant(inner) => variant_as_bool(inner),
        _ => None,
    }
}
