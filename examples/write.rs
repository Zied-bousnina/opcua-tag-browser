//! Reads and writes individual tags.
//!
//! ```text
//! RUST_LOG=info cargo run --example write -- opc.tcp://192.168.201.2:4840 Machine/Axis1/Speed
//! ```

use opcua_tag_browser::Collector;

fn main() -> opcua_tag_browser::Result<()> {
    env_logger::init();

    let mut args = std::env::args().skip(1);
    let endpoint = args
        .next()
        .unwrap_or_else(|| "opc.tcp://localhost:4840".to_string());
    let tag = args.next().unwrap_or_else(|| "Machine/Axis1/Speed".to_string());

    let plc = Collector::new("line1", endpoint).insecure().client()?;

    println!("{} tags available", plc.tags().len());
    println!("{tag} = {}", plc.get(&tag)?);

    // Uncomment to write:
    // plc.set(&tag, 1500.0)?;

    Ok(())
}
