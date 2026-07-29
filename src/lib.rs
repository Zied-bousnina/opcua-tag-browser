//! Turn an OPC UA server's address space into a flat, cacheable tag list.
//!
//! Industrial OPC UA servers expose thousands of nodes in a deep tree. Before
//! you can subscribe to or poll anything, you have to walk that tree, decide
//! which nodes are process data, and store the result so you are not rebrowsing
//! on every restart. This crate does that part.
//!
//! # What it does
//!
//! - Recursive `Browse` / `BrowseNext` with continuation points handled
//! - Cycle detection and a configurable depth limit
//! - Pluggable filtering of server furniture via [`NodeFilter`]
//! - Panics inside the `opcua` crate converted to errors instead of killing threads
//! - Partial-scan reporting, so a failed subtree is visible rather than silent
//! - Optional JSON caching of the resulting tag list
//!
//! # What it does not do
//!
//! Subscriptions, polling, and logging are out of scope. Use [`OpcUaSession::inner`]
//! to reach the underlying `opcua` session for those.
//!
//! # Example
//!
//! ```no_run
//! use opcua_tag_browser::{
//!     connect, ConnectOptions, DefaultNodeFilter, JsonFileTagRepository,
//!     OpcUaNodeBrowser, OpcUaSession, PlcSession, ScanOptions, TagRepository,
//!     TreeScanner,
//! };
//! use opcua_tag_browser::opcua::client::prelude::NodeId;
//! use std::sync::Arc;
//!
//! # fn main() -> opcua_tag_browser::Result<()> {
//! let raw = connect("opc.tcp://192.168.201.2:4840", &ConnectOptions::default())?;
//! let session: Arc<dyn PlcSession> = Arc::new(OpcUaSession::new(raw));
//!
//! let browser = OpcUaNodeBrowser::new(session);
//! let scanner = TreeScanner::new(&browser, &DefaultNodeFilter, ScanOptions::default());
//! let report = scanner.scan(NodeId::objects_folder_id())?;
//!
//! if !report.is_complete() {
//!     eprintln!("{} subtrees were skipped", report.skipped.len());
//! }
//!
//! JsonFileTagRepository::new("plc_tags.json").save(&report.tags)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Feature flags
//!
//! - `json-cache` *(default)* — enables [`JsonFileTagRepository`].

#![deny(missing_docs)]
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
/// The public API exposes `NodeId`, `StatusCode`, and `Variant` from it. Use
/// this re-export rather than adding your own `opcua` dependency, or a version
/// mismatch will produce confusing type errors.
pub use opcua;