//! Turn an OPC UA server's address space into a flat, serializable, cacheable tag list.
//!
//! Industrial OPC UA servers expose thousands of nodes in a deep hierarchy. Before you can
//! subscribe to or poll anything, you have to walk that tree, work out which nodes are actual
//! process data, and store the result so you are not rebrowsing on every restart. This crate
//! does that part, and only that part.
//!
//! # Quick start
//!
//! ```no_run
//! use opcua_tag_browser::opcua::client::prelude::NodeId;
//! use opcua_tag_browser::{
//!     connect, ConnectOptions, DefaultNodeFilter, JsonFileTagRepository, OpcUaNodeBrowser,
//!     OpcUaSession, PlcSession, ScanOptions, TagRepository, TreeScanner,
//! };
//! use std::sync::Arc;
//!
//! # fn main() -> opcua_tag_browser::Result<()> {
//! let raw = connect("opc.tcp://192.168.201.2:4840", &ConnectOptions::default())?;
//! let session: Arc<dyn PlcSession> = Arc::new(OpcUaSession::new(raw));
//!
//! let browser = OpcUaNodeBrowser::new(session.clone());
//! let scanner = TreeScanner::new(&browser, &DefaultNodeFilter, ScanOptions::default());
//! let report = scanner.scan(NodeId::objects_folder_id())?;
//!
//! println!("discovered {} tags", report.tags.len());
//! JsonFileTagRepository::new("plc_tags.json").save(&report.tags)?;
//!
//! let _ = session.close_session_and_delete_subscriptions();
//! # Ok(())
//! # }
//! ```
//!
//! # The pipeline
//!
//! Four stages, each a separate trait so any one can be swapped or faked:
//!
//! ```text
//!   connect          →  PlcSession    open a session, contain panics
//!   browse           →  NodeBrowser   list children, follow continuation points
//!   filter + walk    →  NodeFilter    reject furniture, recurse, detect cycles
//!                       TreeScanner
//!   persist          →  TagRepository cache the flat result
//! ```
//!
//! The traits are the point. [`NodeBrowser`] lets you drive [`TreeScanner`] against a fixture
//! with no server running; [`NodeFilter`] lets vendor-specific junk-name conventions live in
//! your code rather than in a match arm here; [`TagRepository`] lets the cache move to a
//! database without touching the scanner.
//!
//! # Design notes
//!
//! ## Panics are contained, not propagated
//!
//! The `opcua` crate panics on some malformed server responses. In a collector that runs for
//! months, an unwind on a worker thread is a silent outage. Every service call made through
//! [`OpcUaSession`] is wrapped by [`catch_panic`], which converts the unwind into
//! [`StatusCode::BadUnexpectedError`](opcua::client::prelude::StatusCode) so the caller can
//! log it and retry.
//!
//! ## Partial scans are reported, not hidden
//!
//! A scan that returns 2,400 tags because one subtree failed is indistinguishable from a
//! healthy 2,400-tag scan unless the failure is surfaced. [`TreeScanner::scan`] therefore
//! fails outright only when the root itself is unreachable; every deeper failure lands in
//! [`ScanReport::skipped`] and the walk continues.
//!
//! ```no_run
//! # use opcua_tag_browser::ScanReport;
//! # fn demo(report: ScanReport) {
//! if !report.is_complete() {
//!     for (node_id, err) in &report.skipped {
//!         eprintln!("unreadable subtree {node_id}: {err}");
//!     }
//! }
//! # }
//! ```
//!
//! ## Hierarchy is preserved as a path
//!
//! Flattening a tree normally throws away the structure you just spent thousands of round
//! trips discovering. Each [`PlcTag`] keeps its slash-joined browse path from the scan root,
//! so grouping and human-facing tag trees remain possible downstream.
//!
//! ## Logging goes through the `log` facade
//!
//! Nothing is printed to stdout. Scan progress is emitted at `debug` and `trace`, skipped
//! subtrees at `warn`, and the final summary at `info`. Install any `log` implementation to
//! see it:
//!
//! ```text
//! RUST_LOG=opcua_tag_browser=debug cargo run
//! ```
//!
//! # Out of scope
//!
//! Subscriptions, polling, value logging, and reconnection are deliberately absent. They carry
//! tuning that is specific to a deployment — publishing intervals, monitored-item ceilings,
//! batch sizes — and defaults that are correct for one server are misleading for another.
//!
//! Reach the underlying `opcua` session through [`OpcUaSession::inner`] for anything this
//! crate does not cover.
//!
//! # Version compatibility
//!
//! This crate re-exports the [`opcua`] crate it was compiled against. Use that path rather
//! than adding your own `opcua` dependency: the public API exposes `NodeId`, `StatusCode`,
//! `Variant`, and `NodeClass`, and a version mismatch produces type errors that look unrelated
//! to their cause.
//!
//! ```
//! use opcua_tag_browser::opcua::client::prelude::{NodeId, Variant};
//! ```
//!
//! | `opcua-tag-browser` | `opcua` |
//! | --- | --- |
//! | `0.1` | `0.12` |
//!
//! One sharp edge worth knowing: `NodeId::new` is bounded on `T: 'static`, so a borrowed
//! `&str` from a function parameter will not compile. String literals are fine; anything
//! borrowed needs `.to_string()`.
//!
//! ```
//! # use opcua_tag_browser::opcua::client::prelude::NodeId;
//! fn make(id: &str) -> NodeId {
//!     NodeId::new(2, id.to_string()) // `NodeId::new(2, id)` fails with E0521
//! }
//! ```
//!
//! # Feature flags
//!
//! - **`json-cache`** *(default)* — enables [`JsonFileTagRepository`] and pulls in
//!   `serde_json`. Disable it if you supply your own [`TagRepository`].
//!
//! # Minimum supported Rust version
//!
//! 1.75. Raising it is treated as a breaking change and will come with a minor version bump
//! while the crate is pre-1.0.

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
mod variant;

#[cfg(feature = "json-cache")]
mod repository;

pub use browser::{BrowsedNode, NodeBrowser, OpcUaNodeBrowser};
pub use connection::{connect, ConnectOptions};
pub use error::{Error, Result};
pub use filter::{AcceptAll, DefaultNodeFilter, NodeFilter};
pub use scanner::{ScanOptions, ScanReport, TreeScanner};
pub use session::{catch_panic, panic_message, OpcUaSession, PlcSession};
pub use tag::PlcTag;
pub use variant::{format_quality, format_variant};

#[cfg(feature = "json-cache")]
pub use repository::{JsonFileTagRepository, TagRepository};

/// The `opcua` crate this library was compiled against.
///
/// The public API exposes `NodeId`, `StatusCode`, `Variant`, and `NodeClass` from it. Use this
/// re-export rather than declaring your own `opcua` dependency, or a version mismatch will
/// produce type errors that appear unrelated to the real cause.
///
/// ```
/// use opcua_tag_browser::opcua::client::prelude::NodeId;
///
/// let node = NodeId::new(2, "MyTag".to_string());
/// ```
pub use opcua;