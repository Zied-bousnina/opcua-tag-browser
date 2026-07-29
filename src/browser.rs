//! Listing the children of a node, with pagination handled.

use crate::error::{Error, Result};
use crate::session::PlcSession;
use opcua::client::prelude::*;
use std::sync::Arc;

/// A child node discovered while browsing.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BrowsedNode {
    /// Identifier of the discovered node.
    pub node_id: NodeId,
    /// Text of the node's `DisplayName` attribute.
    pub display_name: String,
    /// Class of the discovered node.
    pub node_class: NodeClass,
}
impl BrowsedNode {
    /// Builds a browsed node record.
    ///
    /// Provided so callers outside this crate — test doubles, alternative
    /// [`NodeBrowser`] backends — can construct values despite
    /// `#[non_exhaustive]`.
    pub fn new(
        node_id: NodeId,
        display_name: impl Into<String>,
        node_class: NodeClass,
    ) -> Self {
        Self {
            node_id,
            display_name: display_name.into(),
            node_class,
        }
    }
}

/// Lists the children of a node.
///
/// Implement this to drive [`TreeScanner`](crate::TreeScanner) against a fake
/// address space in tests, or against a cached snapshot.
pub trait NodeBrowser {
    /// Returns every child of `node_id`, following continuation points.
    fn children_of(&self, node_id: &NodeId) -> Result<Vec<BrowsedNode>>;
}

/// A [`NodeBrowser`] backed by a live session.
pub struct OpcUaNodeBrowser {
    session: Arc<dyn PlcSession>,
}

impl OpcUaNodeBrowser {
    /// Creates a browser over the given session.
    pub fn new(session: Arc<dyn PlcSession>) -> Self {
        Self { session }
    }
}

impl NodeBrowser for OpcUaNodeBrowser {
    fn children_of(&self, node_id: &NodeId) -> Result<Vec<BrowsedNode>> {
        let description = BrowseDescription {
            node_id: node_id.clone(),
            browse_direction: BrowseDirection::Forward,
            reference_type_id: ReferenceTypeId::HierarchicalReferences.into(),
            include_subtypes: true,
            node_class_mask: 0,
            result_mask: BrowseResultMask::All as u32,
        };

        let mut children = Vec::new();

        let results = self
            .session
            .browse(&[description])
            .map_err(|status| Error::Browse {
                node_id: node_id.to_string(),
                status,
            })?;

        let Some(results) = results else {
            return Ok(children);
        };

        for result in results {
            if result.status_code.is_bad() {
                return Err(Error::Browse {
                    node_id: node_id.to_string(),
                    status: result.status_code,
                });
            }

            append_references(&result.references, &mut children);

            let mut continuation_point = result.continuation_point.clone();
            while has_more(&continuation_point) {
                let next = self
                    .session
                    .browse_next(false, &[continuation_point.clone()])
                    .map_err(|status| Error::Browse {
                        node_id: node_id.to_string(),
                        status,
                    })?;

                let Some(next_results) = next else { break };

                let mut next_point = ByteString::null();
                for next_result in next_results {
                    if next_result.status_code.is_bad() {
                        return Err(Error::Browse {
                            node_id: node_id.to_string(),
                            status: next_result.status_code,
                        });
                    }
                    append_references(&next_result.references, &mut children);
                    next_point = next_result.continuation_point.clone();
                }
                continuation_point = next_point;
            }
        }

        log::trace!("{} has {} children", node_id, children.len());
        Ok(children)
    }
}

/// Returns true when a continuation point carries a non-empty value.
///
/// A present-but-empty byte string would otherwise loop forever.
fn has_more(point: &ByteString) -> bool {
    point.value.as_ref().is_some_and(|bytes| !bytes.is_empty())
}

/// Converts raw reference descriptions into [`BrowsedNode`] values.
fn append_references(
    references: &Option<Vec<ReferenceDescription>>,
    out: &mut Vec<BrowsedNode>,
) {
    let Some(references) = references else { return };
    for reference in references {
        out.push(BrowsedNode {
            node_id: reference.node_id.node_id.clone(),
            display_name: reference.display_name.text.to_string(),
            node_class: reference.node_class,
        });
    }
}