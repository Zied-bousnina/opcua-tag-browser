//! Error type returned by every fallible operation in this crate.

use opcua::client::prelude::StatusCode;
use std::path::PathBuf;

/// Errors produced while connecting to, browsing, monitoring, or writing to an
/// OPC UA server.
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

    /// A panic raised inside the `opcua` crate was caught and converted.
    #[error("internal panic inside the opcua crate: {0}")]
    InternalPanic(String),

    /// A collector was run without a sink.
    #[error("no sink configured: call Collector::sink before run")]
    MissingSink,

    /// A sink could not be initialized.
    #[error("could not set up sink: {0}")]
    SinkSetup(String),

    /// Credentials would have been sent over an unencrypted channel.
    #[error("refusing to send credentials without encryption: set Security::SignAndEncrypt")]
    InsecureCredentials,

    /// No tag matched the supplied browse path or display name.
    #[error("no tag named {path}")]
    UnknownTag {
        /// The path or name that was looked up.
        path: String,
    },

    /// A node ID string could not be parsed.
    #[error("{node_id} is not a valid node id")]
    BadNodeId {
        /// The string that failed to parse.
        node_id: String,
    },

    /// A `Write` service call failed.
    #[error("write to {node_id} failed: {status}")]
    Write {
        /// Node that was being written.
        node_id: String,
        /// Status code returned by the server.
        status: StatusCode,
    },

    /// A `Read` service call failed.
    #[error("read of {node_id} failed: {status}")]
    Read {
        /// Node that was being read.
        node_id: String,
        /// Status code returned by the server.
        status: StatusCode,
    },

    /// A write was refused because the tag's `AccessLevel` marks it read-only.
    ///
    /// Caught before the round trip: the server would otherwise return
    /// `BadNotWritable` after the request already left.
    #[error("{path} is not writable (AccessLevel does not permit CurrentWrite)")]
    NotWritable {
        /// The path or name that was looked up.
        path: String,
    },
}

impl Error {
    /// Whether retrying the operation that produced this error has any
    /// chance of succeeding.
    ///
    /// Used by [`Collector::run`](crate::Collector::run)'s resilient restart
    /// loop to tell a transient failure (network blip, a slow server) from a
    /// configuration mistake that will fail identically on every attempt —
    /// retrying the latter forever would just spam the log.
    pub fn is_recoverable(&self) -> bool {
        !matches!(self, Error::InsecureCredentials | Error::ClientBuild(_))
    }
}

/// Convenience alias for results returned by this crate.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_errors_are_not_recoverable() {
        assert!(!Error::InsecureCredentials.is_recoverable());
        assert!(!Error::ClientBuild("bad options".to_string()).is_recoverable());
    }

    #[test]
    fn transient_errors_are_recoverable() {
        assert!(Error::Connect {
            endpoint: "opc.tcp://host:4840".to_string(),
            status: StatusCode::BadNotConnected,
        }
        .is_recoverable());
        assert!(Error::InternalPanic("boom".to_string()).is_recoverable());
        assert!(Error::MissingSink.is_recoverable());
    }
}
