//! Error type returned by every fallible operation in this crate.

use opcua::client::prelude::StatusCode;
use std::path::PathBuf;

/// Errors produced while connecting to, browsing, or caching an OPC UA address space.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A `Browse` or `BrowseNext` service call failed for a specific node.
    #[error("browse of node {node_id} failed: {status}")]
    Browse {
        /// String form of the node whose children could not be listed.
        node_id: String,
        /// Status code returned by the server.
        status: StatusCode,
    },

    /// The endpoint could not be reached, or the session handshake failed.
    #[error("could not connect to {endpoint}: {status}")]
    Connect {
        /// Endpoint URL that was attempted.
        endpoint: String,
        /// Status code returned by the server.
        status: StatusCode,
    },

    /// The underlying OPC UA client could not be built from the supplied options.
    #[error("could not build OPC UA client: {0}")]
    ClientBuild(String),

    /// Reading or writing a tag cache file failed.
    #[error("tag cache I/O failed at {path}")]
    CacheIo {
        /// Path of the cache file involved.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// A tag cache file existed but could not be parsed.
    #[cfg(feature = "json-cache")]
    #[error("tag cache at {path} is not valid JSON")]
    CacheFormat {
        /// Path of the cache file involved.
        path: PathBuf,
        /// The underlying deserialization error.
        #[source]
        source: serde_json::Error,
    },

    /// A panic raised inside the `opcua` crate was caught and converted into an error.
    #[error("internal panic inside the opcua crate: {0}")]
    InternalPanic(String),
}

/// Convenience alias for results returned by this crate.
pub type Result<T> = std::result::Result<T, Error>;