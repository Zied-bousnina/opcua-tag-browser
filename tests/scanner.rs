//! Scanner behaviour against a fake address space.

use opcua_tag_browser::opcua::client::prelude::{NodeClass, NodeId};
use opcua_tag_browser::{
    AcceptAll, BrowsedNode, DefaultNodeFilter, NodeBrowser, ScanOptions, TreeScanner,
};
use std::collections::HashMap;

struct FakeBrowser {
    children: HashMap<String, Vec<BrowsedNode>>,
}

impl FakeBrowser {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
        }
    }

    fn with(mut self, parent: &NodeId, children: Vec<BrowsedNode>) -> Self {
        self.children.insert(parent.to_string(), children);
        self
    }
}

impl NodeBrowser for FakeBrowser {
    fn children_of(&self, node_id: &NodeId) -> opcua_tag_browser::Result<Vec<BrowsedNode>> {
        Ok(self
            .children
            .get(&node_id.to_string())
            .cloned()
            .unwrap_or_default())
    }
}

fn node(id: &str, name: &str, class: NodeClass) -> BrowsedNode {
    BrowsedNode::new(NodeId::new(2, id.to_string()), name, class)
}

#[test]
fn collects_variables_and_records_their_path() {
    let root = NodeId::new(2, "root");
    let machine = node("machine", "Machine", NodeClass::Object);
    let speed = node("speed", "Speed", NodeClass::Variable);

    let browser = FakeBrowser::new()
        .with(&root, vec![machine.clone()])
        .with(&machine.node_id, vec![speed]);

    let report = TreeScanner::new(&browser, &AcceptAll, ScanOptions::default())
        .scan(root)
        .expect("scan should succeed");

    assert_eq!(report.tags.len(), 1);
    assert_eq!(report.tags[0].path, "Machine/Speed");
    assert!(report.is_complete());
}

#[test]
fn default_filter_drops_server_furniture() {
    let root = NodeId::new(2, "root");
    let browser = FakeBrowser::new().with(
        &root,
        vec![
            node("icon", "Icon", NodeClass::Variable),
            node("sep", "------", NodeClass::Variable),
            node("real", "Speed", NodeClass::Variable),
        ],
    );

    let report = TreeScanner::new(&browser, &DefaultNodeFilter, ScanOptions::default())
        .scan(root)
        .unwrap();

    assert_eq!(report.tags.len(), 1);
    assert_eq!(report.tags[0].display_name, "Speed");
}

#[test]
fn cyclic_references_terminate() {
    let root = NodeId::new(2, "root");
    let a = node("a", "A", NodeClass::Object);
    let b = node("b", "B", NodeClass::Object);

    let browser = FakeBrowser::new()
        .with(&root, vec![a.clone()])
        .with(&a.node_id, vec![b.clone()])
        .with(&b.node_id, vec![a.clone()]);

    let report = TreeScanner::new(&browser, &AcceptAll, ScanOptions::default())
        .scan(root)
        .unwrap();

    assert!(report.tags.is_empty());
}

#[test]
fn shared_node_is_expanded_under_every_path() {
    // NodeId 9 is reachable as both /F/H and /B/H. The spec requires each
    // browse path be treated as a distinct node.
    let root = NodeId::new(2, "root");
    let f = node("f", "F", NodeClass::Object);
    let b = node("b", "B", NodeClass::Object);
    let shared = node("shared", "H", NodeClass::Object);
    let leaf = node("leaf", "Value", NodeClass::Variable);

    let browser = FakeBrowser::new()
        .with(&root, vec![f.clone(), b.clone()])
        .with(&f.node_id, vec![shared.clone()])
        .with(&b.node_id, vec![shared.clone()])
        .with(&shared.node_id, vec![leaf]);

    let report = TreeScanner::new(&browser, &AcceptAll, ScanOptions::default())
        .scan(root)
        .unwrap();

    let paths: Vec<&str> = report.tags.iter().map(|t| t.path.as_str()).collect();
    assert!(paths.contains(&"F/H/Value"), "got {paths:?}");
    assert!(paths.contains(&"B/H/Value"), "got {paths:?}");
}

#[test]
fn depth_limit_is_respected() {
    let root = NodeId::new(2, "root");
    let level1 = node("l1", "L1", NodeClass::Object);
    let deep = node("deep", "Deep", NodeClass::Variable);

    let browser = FakeBrowser::new()
        .with(&root, vec![level1.clone()])
        .with(&level1.node_id, vec![deep]);

    let mut options = ScanOptions::default();
    options.max_depth = 1;

    let report = TreeScanner::new(&browser, &AcceptAll, options)
        .scan(root)
        .unwrap();

    assert!(report.tags.is_empty());
}