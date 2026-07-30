//! Collects everything a PLC exposes into daily JSONL files.
//!
//! ```text
//! RUST_LOG=info cargo run --example collect --features full -- opc.tcp://192.168.201.2:4840
//! ```

use opcua_tag_browser::Collector;

fn main() -> opcua_tag_browser::Result<()> {
    env_logger::init();

    let endpoint = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "opc.tcp://localhost:4840".to_string());

    Collector::new("line1", endpoint)
        .insecure()
        .jsonl("logs")
        .handle_ctrl_c()
        .force_rescan(std::env::args().any(|a| a == "--rescan"))
        .run()
}
