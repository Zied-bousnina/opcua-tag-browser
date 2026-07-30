//! An OPC UA client for PLC data.
//!
//! Point a [`Collector`] at a PLC and it discovers every tag, subscribes to as
//! many as the server will monitor, polls the rest, reconnects when the link
//! drops, and streams every change to a sink you choose. [`TagClient`] reads
//! and writes individual values on demand.
//!
//! ```no_run
//! use opcua_tag_browser::{Collector, TagChange};
//!
//! # fn main() -> opcua_tag_browser::Result<()> {
//! Collector::new("line1", "opc.tcp://192.168.201.2:4840")
//!     .insecure()
//!     .sink(|e: &TagChange<'_>| println!("{} = {}", e.path(), e.value))
//!     .run()
//! # }
//! ```
//!
//! # Selecting what to collect
//!
//! A PLC may expose thousands of nodes, and servers cap how many can be
//! monitored at once. Selecting fewer than the ceiling means everything arrives
//! by subscription and polling never engages.
//!
//! ```no_run
//! # use opcua_tag_browser::Collector;
//! # fn c() -> Collector { Collector::new("a", "b") }
//! c().only(["Machine/Axis1/Speed", "Machine/Axis1/Position"]);
//! c().matching("Machine/Axis*/Speed");
//! c().select(|tag| tag.path.starts_with("Machine/") && !tag.path.contains("Diag"));
//! ```
//!
//! # Writing values
//!
//! ```no_run
//! use opcua_tag_browser::Collector;
//!
//! # fn main() -> opcua_tag_browser::Result<()> {
//! let plc = Collector::new("line1", "opc.tcp://192.168.201.2:4840")
//!     .insecure()
//!     .client()?;
//!
//! plc.set("Machine/Axis1/Setpoint", 1500.0)?;
//! plc.set("Machine/Enable", true)?;
//!
//! println!("{}", plc.get("Machine/Axis1/Speed")?);
//! # Ok(())
//! # }
//! ```
//!
//! [`TagClient`] is `Clone` and `Send`, so it can be moved into another thread
//! while a collector's event loop runs.
//!
//! # Security
//!
//! [`ConnectOptions::default`] signs and encrypts with `Basic256Sha256` and
//! rejects untrusted server certificates. That fails against a server
//! configured for plaintext, which is deliberate: calling
//! [`Collector::insecure`] records the decision in your source instead of
//! inheriting it from a default.
//!
//! ```
//! use opcua_tag_browser::{ConnectOptions, Security};
//! use opcua_tag_browser::opcua::client::prelude::SecurityPolicy;
//!
//! let secure = ConnectOptions::default()
//!     .security(Security::SignAndEncrypt(SecurityPolicy::Basic256Sha256))
//!     .user_name("collector", "hunter2");
//! ```
//!
//! Sending a username over an unencrypted channel returns
//! [`Error::InsecureCredentials`] rather than transmitting it, and
//! [`Credentials`] redacts passwords in `Debug` output so they cannot reach a
//! log by accident.
//!
//! # Sinks
//!
//! [`TagSink`] is the seam between the crate and your application; the crate
//! decides nothing about where data goes. A closure implements it, and
//! [`sinks::LogSink`] and [`sinks::JsonlSink`] ship ready to use.
//!
//! ```
//! use opcua_tag_browser::{TagChange, TagSink};
//!
//! struct Threshold;
//!
//! impl TagSink for Threshold {
//!     fn tag_changed(&self, event: &TagChange<'_>) {
//!         if event.as_f64().is_some_and(|v| v > 100.0) {
//!             println!("{} is high: {}", event.path(), event.value);
//!         }
//!     }
//! }
//! ```
//!
//! # Shutting down
//!
//! The crate never calls `process::exit`. In a binary,
//! [`Collector::handle_ctrl_c`] wires up a handler; inside a larger
//! application, take a [`CollectorHandle`] and stop it yourself.
//!
//! Closing sessions on exit matters more than it looks. A server does not learn
//! an abandoned session is gone until it times out, and until then its
//! monitored items still count against the budget.
//!
//! # Lower-level pieces
//!
//! [`Collector`] composes parts that stay public. Reach for them when you want
//! the address space but not the monitoring:
//!
//! | Type | Role |
//! | --- | --- |
//! | [`connect`] | open a session |
//! | [`PlcSession`] | session operations, with `opcua` panics contained |
//! | [`NodeBrowser`] | list a node's children, pagination handled |
//! | [`NodeFilter`] | reject server furniture |
//! | [`TreeScanner`] | walk the tree into [`PlcTag`] values |
//! | [`TagSet`] | index tags for lookup by path, name, or glob |
//! | [`TagRepository`] | cache the result |
//!
//! # Feature flags
//!
//! | Feature | Default | Effect |
//! | --- | --- | --- |
//! | `monitoring` | yes | [`Collector`], [`TagClient`], subscriptions, polling |
//! | `json-cache` | yes | [`JsonFileTagRepository`] and tag caching |
//! | `jsonl-sink` | no | [`sinks::JsonlSink`] and [`Collector::jsonl`] |
//! | `ctrl-c` | no | [`Collector::handle_ctrl_c`] |
//! | `full` | no | all of the above |
//!
//! # Version compatibility
//!
//! This crate re-exports the [`opcua`] crate it was built against. Use that
//! path rather than your own `opcua` dependency, or a version mismatch produces
//! type errors that look unrelated to their cause.
//!
//! | `opcua-tag-browser` | `opcua` |
//! | --- | --- |
//! | `0.2` | `0.12` |
//! | `0.1` | `0.12` |
//!
//! # Minimum supported Rust version
//!
//! 1.75. Raising it is a breaking change.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]

mod browser;
mod connection;
mod error;
mod filter;
mod scanner;
mod session;
mod tag;
mod tagset;
mod variant;

#[cfg(feature = "json-cache")]
mod repository;

#[cfg(feature = "monitoring")]
mod collector;
#[cfg(feature = "monitoring")]
mod monitor;
#[cfg(feature = "monitoring")]
mod sink;
#[cfg(feature = "monitoring")]
pub mod sinks;
#[cfg(feature = "monitoring")]
mod writer;

pub use browser::{BrowsedNode, NodeBrowser, OpcUaNodeBrowser};
pub use connection::{connect, ConnectOptions, Credentials, Security};
pub use error::{Error, Result};
pub use filter::{AcceptAll, DefaultNodeFilter, NodeFilter};
pub use scanner::{ScanOptions, ScanReport, TreeScanner};
pub use session::{catch_panic, panic_message, OpcUaSession, PlcSession};
pub use tag::PlcTag;
pub use tagset::TagSet;
pub use variant::{
    format_quality, format_variant, variant_as_bool, variant_as_f64, variant_as_i64,
};

#[cfg(feature = "json-cache")]
pub use repository::{JsonFileTagRepository, TagRepository};

#[cfg(feature = "monitoring")]
pub use collector::{Collector, CollectorHandle};
#[cfg(feature = "monitoring")]
pub use monitor::MonitorOptions;
#[cfg(feature = "monitoring")]
pub use sink::{ConnectionChange, Source, TagChange, TagSink};
#[cfg(feature = "monitoring")]
pub use writer::TagClient;

/// Scans an endpoint and returns its tags.
///
/// Convenience over [`Collector::discover`] for a one-off look at a server.
/// Connects insecurely and does not cache.
///
/// ```no_run
/// let tags = opcua_tag_browser::discover("opc.tcp://192.168.201.2:4840")?;
/// for tag in tags.iter().take(20) {
///     println!("{}", tag.path);
/// }
/// # Ok::<(), opcua_tag_browser::Error>(())
/// ```
#[cfg(all(feature = "monitoring", feature = "json-cache"))]
pub fn discover(endpoint_url: &str) -> Result<TagSet> {
    Collector::new("discover", endpoint_url)
        .insecure()
        .no_cache()
        .discover()
}

/// The `opcua` crate this library was compiled against.
///
/// The public API exposes `NodeId`, `StatusCode`, `Variant`, `NodeClass`, and
/// `SecurityPolicy` from it. Use this re-export rather than declaring your own
/// `opcua` dependency.
///
/// ```
/// use opcua_tag_browser::opcua::client::prelude::NodeId;
///
/// let node = NodeId::new(2, "MyTag".to_string());
/// ```
pub use opcua;
