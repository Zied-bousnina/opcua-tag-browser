//! Scans a server and writes the tag list to JSON.
//!
//! ```text
//! RUST_LOG=info cargo run --example scan_and_dump -- opc.tcp://192.168.200.0:8080
//! ```

use opcua_tag_browser::opcua::client::prelude::NodeId;
use opcua_tag_browser::{
    connect, ConnectOptions, DefaultNodeFilter, JsonFileTagRepository, OpcUaNodeBrowser,
    OpcUaSession, PlcSession, ScanOptions, TagRepository, TreeScanner,
};
use std::sync::Arc;

fn main() -> opcua_tag_browser::Result<()> {
    let endpoint = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "opc.tcp://localhost:8080".to_string());

    let raw = connect(&endpoint, &ConnectOptions::insecure())?;
    let session: Arc<dyn PlcSession> = Arc::new(OpcUaSession::new(raw));

    let browser = OpcUaNodeBrowser::new(session.clone());
    let scanner = TreeScanner::new(&browser, &DefaultNodeFilter, ScanOptions::default());
    let report = scanner.scan(NodeId::objects_folder_id())?;

    println!("discovered {} tags", report.tags.len());
    for (node_id, err) in &report.skipped {
        eprintln!("skipped {}: {}", node_id, err);
    }

    JsonFileTagRepository::new("plc_tags.json").save(&report.tags)?;
    println!("written to plc_tags.json");

    let _ = session.close_session_and_delete_subscriptions();
    Ok(())
}