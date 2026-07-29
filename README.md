# opcua-tag-browser

[![crates.io](https://img.shields.io/crates/v/opcua-tag-browser.svg)](https://crates.io/crates/opcua-tag-browser)
[![docs.rs](https://docs.rs/opcua-tag-browser/badge.svg)](https://docs.rs/opcua-tag-browser)
[![MSRV](https://img.shields.io/badge/MSRV-1.75-blue.svg)](https://blog.rust-lang.org/2023/12/28/Rust-1.75.0.html)
[![license](https://img.shields.io/crates/l/opcua-tag-browser.svg)](#license)

Turn an OPC UA server's address space into a flat, serializable, cacheable tag list.

Industrial OPC UA servers expose thousands of nodes in a deep hierarchy. Before you can
subscribe to or poll anything, you have to walk that tree, work out which nodes are actual
process data, and store the result so you are not rebrowsing on every restart. This crate
does that part, and only that part.

```toml
[dependencies]
opcua-tag-browser = "0.1"
```

## Quick start

```rust,no_run
use opcua_tag_browser::opcua::client::prelude::NodeId;
use opcua_tag_browser::{
    connect, ConnectOptions, DefaultNodeFilter, JsonFileTagRepository, OpcUaNodeBrowser,
    OpcUaSession, PlcSession, ScanOptions, TagRepository, TreeScanner,
};
use std::sync::Arc;

fn main() -> opcua_tag_browser::Result<()> {
    // 1. Open a session.
    let raw = connect("opc.tcp://192.168.201.0:8080", &ConnectOptions::default())?;
    let session: Arc<dyn PlcSession> = Arc::new(OpcUaSession::new(raw));

    // 2. Walk the address space.
    let browser = OpcUaNodeBrowser::new(session.clone());
    let scanner = TreeScanner::new(&browser, &DefaultNodeFilter, ScanOptions::default());
    let report = scanner.scan(NodeId::objects_folder_id())?;

    println!("discovered {} tags", report.tags.len());
    for (node_id, err) in &report.skipped {
        eprintln!("skipped subtree {node_id}: {err}");
    }

    // 3. Cache the result.
    JsonFileTagRepository::new("plc_tags.json").save(&report.tags)?;

    let _ = session.close_session_and_delete_subscriptions();
    Ok(())
}
```

Run the bundled example against your own server:

```bash
RUST_LOG=info cargo run --example scan_and_dump -- opc.tcp://192.168.201.0:8080
```

## What you get

- **Recursive browsing** with `BrowseNext` continuation points handled, including the
  present-but-empty continuation point that otherwise loops forever.
- **Cycle detection** and a configurable depth limit, so a self-referential address space
  terminates instead of exhausting the stack.
- **Pluggable filtering** through [`NodeFilter`]. Any `Fn(&str) -> bool` implements it, so a
  closure works without defining a type.
- **Panic containment.** The `opcua` crate panics on some malformed server responses. Every
  service call here is wrapped, so a panic becomes a `StatusCode` error instead of killing
  your collector thread.
- **Partial-scan reporting.** A scan that returns 2,400 tags because one subtree failed looks
  identical to a healthy 2,400-tag scan unless the failure is surfaced. [`ScanReport`] carries
  both the tags and the subtrees that could not be read.
- **Browse paths.** Each [`PlcTag`] records its slash-joined path from the scan root, so the
  flat list does not throw away the hierarchy you just walked.
- **Optional JSON caching** behind the default `json-cache` feature.

## What is out of scope

Subscriptions, polling, value logging, and reconnection are deliberately not here. They carry
opinionated tuning that belongs to your application, not to a browsing library.

Use [`OpcUaSession::inner`] to reach the underlying `opcua` session for those:

```rust,no_run
# use opcua_tag_browser::{connect, ConnectOptions, OpcUaSession};
# fn main() -> opcua_tag_browser::Result<()> {
let raw = connect("opc.tcp://localhost:4840", &ConnectOptions::default())?;
let session = OpcUaSession::new(raw);

// Anything this crate does not cover, do directly on the inner session.
let inner = session.inner();
let subscription_id = inner.write().create_subscription(
    /* publishing_interval */ 100.0,
    /* lifetime_count      */ 100,
    /* max_keep_alive      */ 10,
    /* max_notifications   */ 0,
    /* priority            */ 0,
    /* publishing_enabled  */ true,
    /* callback            */ todo!(),
)?;
# Ok(())
# }
```

## Guide

### Filtering

[`DefaultNodeFilter`] drops nodes named `Icon` or `Server`, and separator rows containing
`------`. Those are conventions seen on Siemens and comparable servers. If your vendor litters
the address space differently, pass a closure:

```rust
use opcua_tag_browser::{NodeFilter, ScanOptions, TreeScanner};

let only_process_data = |name: &str| {
    !name.starts_with('_') && !name.eq_ignore_ascii_case("Diagnostics")
};

assert!(only_process_data.accepts("Speed"));
assert!(!only_process_data.accepts("_internal"));
```

[`AcceptAll`] keeps everything, which is what you want when you plan to filter downstream.

### Scan options

```rust
use opcua_tag_browser::ScanOptions;

let mut options = ScanOptions::default();
options.max_depth = 20;                  // default 12
options.descend_into_variables = false;  // top-level variables only
```

`descend_into_variables` matters for structured tags: a UDT or struct exposes its members as
child `Variable` nodes, so leaving it on (the default) is how you reach individual fields.
Turn it off when you only want the container.

### Reading the scan report

```rust,no_run
# use opcua_tag_browser::ScanReport;
# fn demo(report: ScanReport) {
if report.is_complete() {
    println!("clean scan: {} tags", report.tags.len());
} else {
    eprintln!("{} subtrees unreadable", report.skipped.len());
    for (node_id, err) in &report.skipped {
        eprintln!("  {node_id}: {err}");
    }
}

let tags = report.into_tags();
# }
```

A scan fails outright only when the root node itself cannot be browsed. Everything deeper is
recorded and the walk continues, because a single unreadable subtree is a normal condition on
a large server and should not discard 2,000 good tags.

### Caching

```rust,no_run
use opcua_tag_browser::{JsonFileTagRepository, TagRepository};

# fn demo() -> opcua_tag_browser::Result<()> {
let cache = JsonFileTagRepository::new("plc_tags.json");

let tags = if cache.exists() {
    cache.load()?
} else {
    let scanned = todo!("run a scan");
    cache.save(&scanned)?;
    scanned
};
# Ok(())
# }
```

[`TagRepository`] is a trait, so backing the cache with a database or object store means
implementing three methods, not rewriting the scanner.

### Testing without a server

[`NodeBrowser`] is the seam. Implement it over a fixture and the whole scanner is testable
offline:

```rust
use opcua_tag_browser::opcua::client::prelude::{NodeClass, NodeId};
use opcua_tag_browser::{AcceptAll, BrowsedNode, NodeBrowser, ScanOptions, TreeScanner};
use std::collections::HashMap;

struct FakeBrowser(HashMap<String, Vec<BrowsedNode>>);

impl NodeBrowser for FakeBrowser {
    fn children_of(&self, node_id: &NodeId) -> opcua_tag_browser::Result<Vec<BrowsedNode>> {
        Ok(self.0.get(&node_id.to_string()).cloned().unwrap_or_default())
    }
}

let root = NodeId::new(2, "root");
let mut fixture = HashMap::new();
fixture.insert(
    root.to_string(),
    vec![BrowsedNode::new(
        NodeId::new(2, "speed".to_string()),
        "Speed",
        NodeClass::Variable,
    )],
);

let browser = FakeBrowser(fixture);
let report = TreeScanner::new(&browser, &AcceptAll, ScanOptions::default())
    .scan(root)
    .unwrap();

assert_eq!(report.tags.len(), 1);
assert_eq!(report.tags[0].path, "Speed");
```

Note `NodeId::new(2, "speed".to_string())`. The `opcua` crate bounds that constructor on
`T: 'static`, so a borrowed `&str` from a function parameter will not compile. String literals
are fine; anything borrowed needs `.to_string()`.

## Version compatibility

This crate re-exports the `opcua` crate it was built against:

```rust
use opcua_tag_browser::opcua::client::prelude::{NodeId, StatusCode, Variant};
```

Use that path rather than adding your own `opcua` dependency. The public API exposes `NodeId`,
`StatusCode`, `Variant`, and `NodeClass`, so a version mismatch produces type errors that look
unrelated to the real cause.

| `opcua-tag-browser` | `opcua` |
| --- | --- |
| `0.1` | `0.12` |

## Feature flags

| Feature | Default | Effect |
| --- | --- | --- |
| `json-cache` | yes | Enables [`JsonFileTagRepository`] and the `serde_json` dependency. |

Disable default features if you supply your own [`TagRepository`]:

```toml
opcua-tag-browser = { version = "0.1", default-features = false }
```

## Requirements

- Rust 1.75 or later.
- OpenSSL, because `opcua` links against it. On Debian and Ubuntu that is
  `libssl-dev` plus `pkg-config`; on Windows the `vcpkg` or prebuilt route documented by the
  [`openssl` crate](https://docs.rs/openssl) applies.

## Contributing

Issues and pull requests are welcome at
[github.com/Zied-bousnina/opcua-tag-browser](https://github.com/Zied-bousnina/opcua-tag-browser).

Server-specific quirks are especially useful. Most of what this crate handles — pagination
edge cases, junk node names, the fact that servers cap monitored items well below what they
advertise — was learned against one vendor's PLC, and reports from other hardware are how that
gets generalized.

```bash
cargo test --all-features
cargo clippy --all-features -- -D warnings
cargo fmt --check
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

The `opcua` dependency is MPL-2.0. MPL is file-level copyleft, so depending on it does not
place any obligation on your own source files.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion
in this crate by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.

[`AcceptAll`]: https://docs.rs/opcua-tag-browser/latest/opcua_tag_browser/struct.AcceptAll.html
[`DefaultNodeFilter`]: https://docs.rs/opcua-tag-browser/latest/opcua_tag_browser/struct.DefaultNodeFilter.html
[`JsonFileTagRepository`]: https://docs.rs/opcua-tag-browser/latest/opcua_tag_browser/struct.JsonFileTagRepository.html
[`NodeBrowser`]: https://docs.rs/opcua-tag-browser/latest/opcua_tag_browser/trait.NodeBrowser.html
[`NodeFilter`]: https://docs.rs/opcua-tag-browser/latest/opcua_tag_browser/trait.NodeFilter.html
[`OpcUaSession::inner`]: https://docs.rs/opcua-tag-browser/latest/opcua_tag_browser/struct.OpcUaSession.html#method.inner
[`PlcTag`]: https://docs.rs/opcua-tag-browser/latest/opcua_tag_browser/struct.PlcTag.html
[`ScanReport`]: https://docs.rs/opcua-tag-browser/latest/opcua_tag_browser/struct.ScanReport.html
[`TagRepository`]: https://docs.rs/opcua-tag-browser/latest/opcua_tag_browser/trait.TagRepository.html
