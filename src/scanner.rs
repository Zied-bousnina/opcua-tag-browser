//! Walking an address space into a flat tag list.

use crate::browser::{BrowsedNode, NodeBrowser};
use crate::error::{Error, Result};
use crate::filter::NodeFilter;
use crate::tag::PlcTag;
use opcua::client::prelude::{NodeClass, NodeId};

/// Tuning knobs for a scan.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ScanOptions {
    /// Maximum recursion depth below the root node.
    ///
    /// Guards against pathological address spaces. Twelve levels covers a
    /// typical PLC data-block hierarchy with room to spare.
    pub max_depth: usize,

    /// Whether to descend into the children of `Variable` nodes.
    ///
    /// Structured tags expose their members as child variables, so this is
    /// usually what you want. Turn it off to collect only top-level variables.
    pub descend_into_variables: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            max_depth: 12,
            descend_into_variables: true,
        }
    }
}

impl ScanOptions {
    /// Sets the maximum recursion depth below the root node.
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// Sets whether to descend into the children of `Variable` nodes.
    pub fn descend_into_variables(mut self, yes: bool) -> Self {
        self.descend_into_variables = yes;
        self
    }
}

/// The outcome of a scan, including the subtrees that could not be read.
///
/// Partial results are reported rather than silently dropped: a scan that
/// returns 2,400 tags because one subtree failed looks identical to a healthy
/// 2,400-tag scan unless the failure is surfaced.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct ScanReport {
    /// Every variable node discovered, in browse order.
    pub tags: Vec<PlcTag>,
    /// Subtrees that could not be browsed, paired with the reason.
    pub skipped: Vec<(String, Error)>,
}

impl ScanReport {
    /// Returns `true` when no subtree was skipped.
    pub fn is_complete(&self) -> bool {
        self.skipped.is_empty()
    }

    /// Consumes the report and returns just the tags.
    pub fn into_tags(self) -> Vec<PlcTag> {
        self.tags
    }
}

/// Recursively walks an address space, collecting variable nodes as tags.
pub struct TreeScanner<'a> {
    browser: &'a dyn NodeBrowser,
    filter: &'a dyn NodeFilter,
    options: ScanOptions,
}

impl<'a> TreeScanner<'a> {
    /// Creates a scanner over the given browser and filter.
    pub fn new(
        browser: &'a dyn NodeBrowser,
        filter: &'a dyn NodeFilter,
        options: ScanOptions,
    ) -> Self {
        Self {
            browser,
            filter,
            options,
        }
    }

    /// Scans everything reachable from `root`.
    ///
    /// Fails only if `root` itself cannot be browsed; deeper failures are
    /// recorded in [`ScanReport::skipped`].
    ///
    /// ```no_run
    /// use opcua_tag_browser::{
    ///     connect, ConnectOptions, DefaultNodeFilter, OpcUaNodeBrowser,
    ///     OpcUaSession, PlcSession, ScanOptions, TreeScanner,
    /// };
    /// use opcua_tag_browser::opcua::client::prelude::NodeId;
    /// use std::sync::Arc;
    ///
    /// # fn main() -> opcua_tag_browser::Result<()> {
    /// let raw = connect("opc.tcp://localhost:4840", &ConnectOptions::default())?;
    /// let session: Arc<dyn PlcSession> = Arc::new(OpcUaSession::new(raw));
    /// let browser = OpcUaNodeBrowser::new(session);
    /// let report = TreeScanner::new(&browser, &DefaultNodeFilter, ScanOptions::default())
    ///     .scan(NodeId::objects_folder_id())?;
    ///
    /// println!("{} tags", report.tags.len());
    /// # Ok(())
    /// # }
    /// ```
    pub fn scan(&self, root: NodeId) -> Result<ScanReport> {
        let children = self.browser.children_of(&root)?;

        let mut report = ScanReport::default();
        let mut ancestors = vec![root.to_string()];

        self.walk(children, "", 1, &mut ancestors, &mut report);

        log::info!(
            "scan finished: {} tags, {} subtrees skipped",
            report.tags.len(),
            report.skipped.len()
        );
        Ok(report)
    }

    /// Processes one level of children and recurses.
    fn walk(
        &self,
        children: Vec<BrowsedNode>,
        parent_path: &str,
        depth: usize,
        ancestors: &mut Vec<String>,
        report: &mut ScanReport,
    ) {
        if depth > self.options.max_depth {
            return;
        }

        for child in children {
            if !self.filter.accepts(&child.display_name) {
                continue;
            }

            let path = join_path(parent_path, &child.display_name);

            let recurse = match child.node_class {
                NodeClass::Variable => {
                    report.tags.push(PlcTag::new(
                        child.display_name.clone(),
                        child.node_id.to_string(),
                        format!("{:?}", child.node_class),
                        path.clone(),
                    ));
                    self.options.descend_into_variables
                }
                NodeClass::Object => true,
                _ => false,
            };

            if !recurse {
                continue;
            }

            // Guard against reference cycles in the address space.
            let key = child.node_id.to_string();
            if ancestors.contains(&key) {
                log::trace!("cycle at {key}, not descending");
                continue;
            }

            ancestors.push(key.clone());
            match self.browser.children_of(&child.node_id) {
                Ok(grandchildren) => {
                    self.walk(grandchildren, &path, depth + 1, ancestors, report)
                }
                Err(e) => {
                    log::warn!("skipping subtree {key}: {e}");
                    report.skipped.push((key, e));
                }
            }
            ancestors.pop();
        }
    }
}

/// Joins a parent browse path and a child name with `/`.
fn join_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", parent, name)
    }
}