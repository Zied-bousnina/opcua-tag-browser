//! Opening a client session against an endpoint.

use crate::error::{Error, Result};
use opcua::client::prelude::*;
use opcua::sync::RwLock;
use std::sync::Arc;

/// Options for opening a client session.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ConnectOptions {
    /// Application name presented to the server during the handshake.
    pub application_name: String,
    /// Application URI presented to the server. Must match the certificate's URI.
    pub application_uri: String,
    /// Session keep-alive timeout in milliseconds.
    ///
    /// Bounds how long the server holds an abandoned session, and with it any
    /// monitored items that still count against the server's budget.
    pub session_timeout_ms: u32,
    /// Whether to generate a self-signed keypair if none exists on disk.
    pub create_sample_keypair: bool,
    /// Whether to accept any server certificate without validation.
    pub trust_server_certs: bool,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            application_name: "opcua-tag-browser".to_string(),
            application_uri: "urn:opcua-tag-browser".to_string(),
            session_timeout_ms: 30_000,
            create_sample_keypair: true,
            trust_server_certs: true,
        }
    }
}

impl ConnectOptions {
    /// Sets the application name.
    pub fn application_name(mut self, name: impl Into<String>) -> Self {
        self.application_name = name.into();
        self
    }

    /// Sets the application URI.
    pub fn application_uri(mut self, uri: impl Into<String>) -> Self {
        self.application_uri = uri.into();
        self
    }

    /// Sets the session keep-alive timeout in milliseconds.
    pub fn session_timeout_ms(mut self, ms: u32) -> Self {
        self.session_timeout_ms = ms;
        self
    }
}

/// Connects to an endpoint anonymously with no message security.
///
/// This targets the common case on an isolated industrial network. For secured
/// endpoints, build the client yourself and wrap the session in
/// [`OpcUaSession`](crate::OpcUaSession).
///
/// ```no_run
/// use opcua_tag_browser::{connect, ConnectOptions};
///
/// # fn main() -> opcua_tag_browser::Result<()> {
/// let session = connect("opc.tcp://192.168.201.0:8080", &ConnectOptions::default())?;
/// # Ok(())
/// # }
/// ```
pub fn connect(
    endpoint_url: &str,
    options: &ConnectOptions,
) -> Result<Arc<RwLock<Session>>> {
    let mut client = ClientBuilder::new()
        .application_name(options.application_name.clone())
        .application_uri(options.application_uri.clone())
        .create_sample_keypair(options.create_sample_keypair)
        .trust_server_certs(options.trust_server_certs)
        .session_timeout(options.session_timeout_ms)
        .client()
        .ok_or_else(|| {
            Error::ClientBuild("ClientBuilder returned no client".to_string())
        })?;

    log::debug!("connecting to {}", endpoint_url);

    let session = client
        .connect_to_endpoint(
            (
                endpoint_url,
                SecurityPolicy::None.to_str(),
                MessageSecurityMode::None,
                UserTokenPolicy::anonymous(),
            ),
            IdentityToken::Anonymous,
        )
        .map_err(|status| Error::Connect {
            endpoint: endpoint_url.to_string(),
            status,
        })?;

    log::info!("connected to {}", endpoint_url);
    Ok(session)
}