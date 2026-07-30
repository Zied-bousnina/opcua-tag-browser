//! Writing values back to the PLC.

use crate::error::{Error, Result};
use crate::session::PlcSession;
use crate::tagset::TagSet;
use crate::variant::format_variant;
use opcua::client::prelude::*;
use std::str::FromStr;
use std::sync::Arc;

/// Reads and writes individual tag values on demand.
///
/// Obtained from [`Collector::client`](crate::Collector::client). Cloneable and
/// `Send`, so it can be moved into another thread while the collector's event
/// loop runs.
///
/// ```no_run
/// # use opcua_tag_browser::TagClient;
/// # fn demo(plc: TagClient) -> opcua_tag_browser::Result<()> {
/// plc.set("Machine/Axis1/Setpoint", 1500.0)?;
/// plc.set("Machine/Enable", true)?;
///
/// let speed = plc.get("Machine/Axis1/Speed")?;
/// println!("speed is {speed}");
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct TagClient {
    session: Arc<dyn PlcSession>,
    tags: Arc<TagSet>,
}

impl TagClient {
    pub(crate) fn new(session: Arc<dyn PlcSession>, tags: Arc<TagSet>) -> Self {
        Self { session, tags }
    }

    /// Writes one value, addressing the tag by browse path or display name.
    pub fn set(&self, path: &str, value: impl Into<Variant>) -> Result<()> {
        let node_id = self.resolve(path)?;
        self.set_node(&node_id, value)
    }

    /// Writes one value by raw node ID.
    pub fn set_node(&self, node_id: &str, value: impl Into<Variant>) -> Result<()> {
        let parsed = NodeId::from_str(node_id).map_err(|_| Error::BadNodeId {
            node_id: node_id.to_string(),
        })?;

        let request = WriteValue {
            node_id: parsed,
            attribute_id: AttributeId::Value as u32,
            index_range: UAString::null(),
            value: DataValue::value_only(value.into()),
        };

        let statuses = self
            .session
            .write(&[request])
            .map_err(|status| Error::Write {
                node_id: node_id.to_string(),
                status,
            })?;

        // An overall Ok does not mean the individual write succeeded.
        match statuses.first() {
            Some(status) if status.is_good() => Ok(()),
            Some(status) => Err(Error::Write {
                node_id: node_id.to_string(),
                status: *status,
            }),
            None => Err(Error::Write {
                node_id: node_id.to_string(),
                status: StatusCode::BadUnexpectedError,
            }),
        }
    }

    /// Writes several values in one service call.
    ///
    /// Returns one status per input, in order. Cheaper than repeated
    /// [`set`](Self::set) calls, and the server applies them together.
    ///
    /// ```no_run
    /// # use opcua_tag_browser::TagClient;
    /// # use opcua_tag_browser::opcua::client::prelude::Variant;
    /// # fn demo(plc: TagClient) -> opcua_tag_browser::Result<()> {
    /// let results = plc.set_many(&[
    ///     ("Machine/Axis1/Setpoint", Variant::Double(1500.0)),
    ///     ("Machine/Enable", Variant::Boolean(true)),
    /// ])?;
    /// assert!(results.iter().all(|s| s.is_good()));
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_many(&self, values: &[(&str, Variant)]) -> Result<Vec<StatusCode>> {
        let mut requests = Vec::with_capacity(values.len());

        for (path, value) in values {
            let node_id = self.resolve(path)?;
            let parsed = NodeId::from_str(&node_id).map_err(|_| Error::BadNodeId {
                node_id: node_id.clone(),
            })?;

            requests.push(WriteValue {
                node_id: parsed,
                attribute_id: AttributeId::Value as u32,
                index_range: UAString::null(),
                value: DataValue::value_only(value.clone()),
            });
        }

        self.session.write(&requests).map_err(|status| Error::Write {
            node_id: values.first().map(|(p, _)| p.to_string()).unwrap_or_default(),
            status,
        })
    }

    /// Reads one value as a display string.
    pub fn get(&self, path: &str) -> Result<String> {
        Ok(format_variant(&self.get_variant(path)?))
    }

    /// Reads one value as a raw [`Variant`].
    ///
    /// Use the `variant_as_*` helpers to interpret it.
    pub fn get_variant(&self, path: &str) -> Result<Variant> {
        let node_id = self.resolve(path)?;
        let parsed = NodeId::from_str(&node_id).map_err(|_| Error::BadNodeId {
            node_id: node_id.clone(),
        })?;

        let values = self
            .session
            .read(
                &[ReadValueId::from(parsed)],
                TimestampsToReturn::Neither,
                0.0,
            )
            .map_err(|status| Error::Read {
                node_id: node_id.clone(),
                status,
            })?;

        values
            .into_iter()
            .next()
            .and_then(|dv| dv.value)
            .ok_or(Error::Read {
                node_id,
                status: StatusCode::BadNoData,
            })
    }

    /// The tags this client can address.
    pub fn tags(&self) -> &TagSet {
        &self.tags
    }

    /// Whether the underlying session is connected.
    pub fn is_connected(&self) -> bool {
        self.session.is_connected()
    }

    /// Turns a browse path, display name, or node ID into a node ID.
    fn resolve(&self, path: &str) -> Result<String> {
        if let Some(tag) = self.tags.find(path) {
            return Ok(tag.node_id.clone());
        }
        // Fall back to treating the input as a node ID directly, so callers can
        // address nodes that were filtered out of the scan.
        if NodeId::from_str(path).is_ok() {
            return Ok(path.to_string());
        }
        Err(Error::UnknownTag {
            path: path.to_string(),
        })
    }
}
