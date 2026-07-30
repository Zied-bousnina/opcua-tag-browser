//! Lookup and glob matching over a scanned tag list.

use opcua_tag_browser::{PlcTag, TagSet};

fn fixture() -> TagSet {
    TagSet::new(vec![
        PlcTag::new("Speed", "ns=2;s=a", "Variable", "Machine/Axis1/Speed"),
        PlcTag::new("Position", "ns=2;s=b", "Variable", "Machine/Axis1/Position"),
        PlcTag::new("Speed", "ns=2;s=c", "Variable", "Machine/Axis2/Speed"),
        PlcTag::new("Temp", "ns=2;s=d", "Variable", "Oven/Temp"),
    ])
}

#[test]
fn finds_by_path_and_by_name() {
    let tags = fixture();

    assert_eq!(tags.find("Machine/Axis2/Speed").unwrap().node_id, "ns=2;s=c");
    // Ambiguous display name resolves to the first occurrence.
    assert_eq!(tags.find("Speed").unwrap().node_id, "ns=2;s=a");
    assert!(tags.find("Nonexistent").is_none());
}

#[test]
fn finds_by_node_id() {
    assert_eq!(
        fixture().find_by_node_id("ns=2;s=d").unwrap().path,
        "Oven/Temp"
    );
}

#[test]
fn glob_matches_across_separators() {
    let tags = fixture();

    assert_eq!(tags.matching("Machine/*").len(), 3);
    assert_eq!(tags.matching("*/Speed").len(), 2);
    assert_eq!(tags.matching("Machine/Axis?/Speed").len(), 2);
    assert_eq!(tags.matching("Oven/*").len(), 1);
    assert_eq!(tags.matching("*").len(), 4);
    assert_eq!(tags.matching("Nothing/*").len(), 0);
}

#[test]
fn children_are_direct_only() {
    let tags = fixture();

    // Axis1 and Axis2 are nested one level deeper, so Machine has no direct
    // variable children.
    assert_eq!(tags.children_of("Machine").len(), 0);
    assert_eq!(tags.children_of("Machine/Axis1").len(), 2);
}

#[test]
fn derefs_to_slice() {
    let tags = fixture();
    assert_eq!(tags.iter().filter(|t| t.display_name == "Speed").count(), 2);
}
