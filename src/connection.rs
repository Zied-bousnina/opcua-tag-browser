//! Opening a client session against an endpoint.

use crate::error::{Error, Result};
use opcua::client::prelude::*;
use opcua::sync::RwLock;
use std::fmt;
use std::sync::Arc;

/// Message security applied to a session.
///
/// [`Security::None`] leaves traffic in plaintext. The other two sign, and in
/// the encrypting case also encrypt, using the named policy.
// `SecurityPolicy` is not `Eq`, so neither is this.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Security {
    /// No signing and no encryption. Anyone on the network can read the traffic.
    None,
    /// Messages are signed but sent in the clear.
    Sign(SecurityPolicy),
    /// Messages are signed and encrypted.
    SignAndEncrypt(SecurityPolicy),
}

impl Security {
    /// Returns the security policy this mode uses.
    pub fn policy(&self) -> SecurityPolicy {
        match self {
            Security::None => SecurityPolicy::None,
            Security::Sign(policy) | Security::SignAndEncrypt(policy) => *policy,
        }
    }

    /// Returns the message security mode this maps to on the wire.
    pub fn mode(&self) -> MessageSecurityMode {
        match self {
            Security::None => MessageSecurityMode::None,
            Security::Sign(_) => MessageSecurityMode::Sign,
            Security::SignAndEncrypt(_) => MessageSecurityMode::SignAndEncrypt,
        }
    }

    /// Whether traffic is encrypted, and so safe to carry a password.
    pub fn is_encrypted(&self) -> bool {
        matches!(self, Security::SignAndEncrypt(_))
    }
}

/// How the client identifies itself to the server.
///
/// The `Debug` implementation redacts the password so it cannot reach a log by
/// accident.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Credentials {
    /// No user identity; the server decides what an anonymous client may do.
    Anonymous,
    /// A user name and password, sent only over an encrypted channel.
    UserName {
        /// The user name.
        user: String,
        /// The password. Never appears in `Debug` output.
        password: String,
        /// The server's identifier for its user-name token policy.
        ///
        /// Servers advertise this in their endpoint description and reject a
        /// value they do not recognise. The default suits most servers; the
        /// `opcua` crate's own server uses `userpass_rsa_15` under encryption.
        policy_id: String,
    },
}

impl Credentials {
    /// Default policy identifier used for user-name authentication.
    pub const DEFAULT_USER_NAME_POLICY_ID: &'static str = "username";

    /// Creates user-name credentials with the default policy identifier.
    pub fn user_name(user: impl Into<String>, password: impl Into<String>) -> Self {
        Credentials::UserName {
            user: user.into(),
            password: password.into(),
            policy_id: Self::DEFAULT_USER_NAME_POLICY_ID.to_string(),
        }
    }

    /// Overrides the policy identifier sent with user-name credentials.
    ///
    /// A no-op on [`Credentials::Anonymous`].
    pub fn policy_id(mut self, id: impl Into<String>) -> Self {
        if let Credentials::UserName { policy_id, .. } = &mut self {
            *policy_id = id.into();
        }
        self
    }

    /// Returns the token policy the endpoint description should advertise.
    fn token_policy(&self) -> UserTokenPolicy {
        match self {
            Credentials::Anonymous => UserTokenPolicy::anonymous(),
            Credentials::UserName { policy_id, .. } => UserTokenPolicy {
                policy_id: policy_id.as_str().into(),
                token_type: UserTokenType::UserName,
                issued_token_type: UAString::null(),
                issuer_endpoint_url: UAString::null(),
                security_policy_uri: UAString::null(),
            },
        }
    }

    /// Converts to the `opcua` identity token.
    fn identity_token(&self) -> IdentityToken {
        match self {
            Credentials::Anonymous => IdentityToken::Anonymous,
            Credentials::UserName { user, password, .. } => {
                IdentityToken::UserName(user.clone(), password.clone())
            }
        }
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Credentials::Anonymous => f.write_str("Anonymous"),
            Credentials::UserName {
                user, policy_id, ..
            } => f
                .debug_struct("UserName")
                .field("user", user)
                .field("password", &"<redacted>")
                .field("policy_id", policy_id)
                .finish(),
        }
    }
}

/// Options for opening a client session.
///
/// [`Default`] signs and encrypts with `Basic256Sha256` and rejects untrusted
/// server certificates. Use [`insecure`](Self::insecure) for a plaintext server
/// on an isolated network; naming it keeps the choice visible in your source.
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
    /// Whether to verify server certificates against the trust store.
    pub verify_server_certs: bool,
    /// Message security applied to the session.
    pub security: Security,
    /// How the client identifies itself.
    pub credentials: Credentials,
    /// How many times the underlying `opcua` session retries a dropped
    /// connection before giving up, or `-1` for no limit.
    ///
    /// This governs the built-in reconnect loop `Collector::run` relies on:
    /// when the connection drops, `opcua` reconnects and re-attaches existing
    /// subscriptions on its own, without this crate's involvement. A finite
    /// limit bounds how long `CollectorHandle::stop` can take to take effect
    /// while a reconnect is in progress, at the cost of eventually giving up
    /// on a long outage (`Collector`'s own restart loop then takes over).
    pub session_retry_limit: i32,
    /// Delay between reconnect attempts.
    ///
    /// Floored at 500ms by the underlying `opcua` crate.
    pub session_retry_interval: std::time::Duration,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            application_name: "opcua-tag-browser".to_string(),
            application_uri: "urn:opcua-tag-browser".to_string(),
            session_timeout_ms: 30_000,
            create_sample_keypair: true,
            trust_server_certs: false,
            verify_server_certs: true,
            security: Security::SignAndEncrypt(SecurityPolicy::Basic256Sha256),
            credentials: Credentials::Anonymous,
            session_retry_limit: 20,
            session_retry_interval: std::time::Duration::from_secs(2),
        }
    }
}

impl ConnectOptions {
    /// Connects with no encryption or authentication, trusting any certificate.
    ///
    /// Appropriate on an isolated machine network with no PKI. Anyone able to
    /// see the traffic can read and forge it.
    ///
    /// ```
    /// use opcua_tag_browser::{ConnectOptions, Security};
    ///
    /// assert_eq!(ConnectOptions::insecure().security, Security::None);
    /// ```
    pub fn insecure() -> Self {
        Self {
            trust_server_certs: true,
            verify_server_certs: false,
            security: Security::None,
            credentials: Credentials::Anonymous,
            ..Self::default()
        }
    }

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

    /// Sets the message security applied to the session.
    pub fn security(mut self, security: Security) -> Self {
        self.security = security;
        self
    }

    /// Authenticates with a user name and password.
    ///
    /// The password is only ever sent over an encrypted channel: with any other
    /// [`Security`], [`connect`] fails with [`Error::InsecureCredentials`]
    /// rather than transmitting it.
    pub fn user_name(mut self, user: impl Into<String>, password: impl Into<String>) -> Self {
        self.credentials = Credentials::user_name(user, password);
        self
    }

    /// Sets the credentials directly.
    pub fn credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = credentials;
        self
    }

    /// Accepts any server certificate without validating it.
    pub fn trust_server_certs(mut self, yes: bool) -> Self {
        self.trust_server_certs = yes;
        self
    }

    /// Sets how many times a dropped connection is retried before giving up,
    /// or `-1` for no limit.
    ///
    /// Values below `-1` are clamped to `-1`: the underlying `opcua` crate
    /// panics on them rather than erroring, so this crate does not forward an
    /// invalid value.
    pub fn session_retry_limit(mut self, limit: i32) -> Self {
        self.session_retry_limit = limit.max(-1);
        self
    }

    /// Sets the delay between reconnect attempts. Floored at 500ms by the
    /// underlying `opcua` crate.
    pub fn session_retry_interval(mut self, interval: std::time::Duration) -> Self {
        self.session_retry_interval = interval;
        self
    }
}

/// Connects to an endpoint using the supplied options.
///
/// ```no_run
/// use opcua_tag_browser::{connect, ConnectOptions};
///
/// # fn main() -> opcua_tag_browser::Result<()> {
/// let session = connect("opc.tcp://192.168.201.0:8080", &ConnectOptions::insecure())?;
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// Returns [`Error::InsecureCredentials`] if credentials were configured
/// without encryption, [`Error::ClientBuild`] if the options do not produce a
/// usable client, and [`Error::Connect`] if the handshake fails.
pub fn connect(endpoint_url: &str, options: &ConnectOptions) -> Result<Arc<RwLock<Session>>> {
    // A password on an unencrypted channel is readable by anyone on the wire.
    // Failing here is louder than leaking it and hoping nobody looked.
    if !matches!(options.credentials, Credentials::Anonymous) && !options.security.is_encrypted() {
        return Err(Error::InsecureCredentials);
    }

    let mut client = ClientBuilder::new()
        .application_name(options.application_name.clone())
        .application_uri(options.application_uri.clone())
        .create_sample_keypair(options.create_sample_keypair)
        .trust_server_certs(options.trust_server_certs)
        .verify_server_certs(options.verify_server_certs)
        .session_timeout(options.session_timeout_ms)
        .session_retry_limit(options.session_retry_limit)
        .session_retry_interval(options.session_retry_interval.as_millis() as u32)
        .client()
        .ok_or_else(|| Error::ClientBuild("ClientBuilder returned no client".to_string()))?;

    log::debug!(
        "connecting to {} with {:?} as {:?}",
        endpoint_url,
        options.security,
        options.credentials
    );

    let endpoint: EndpointDescription = (
        endpoint_url,
        options.security.policy().to_str(),
        options.security.mode(),
        options.credentials.token_policy(),
    )
        .into();

    let session = client
        .connect_to_endpoint(endpoint, options.credentials.identity_token())
        .map_err(|status| Error::Connect {
            endpoint: endpoint_url.to_string(),
            status,
        })?;

    log::info!("connected to {}", endpoint_url);
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_encrypted_and_verifies_certs() {
        let options = ConnectOptions::default();
        assert!(options.security.is_encrypted());
        assert!(!options.trust_server_certs);
        assert!(options.verify_server_certs);
    }

    #[test]
    fn insecure_is_plaintext_and_anonymous() {
        let options = ConnectOptions::insecure();
        assert_eq!(options.security, Security::None);
        assert_eq!(options.credentials, Credentials::Anonymous);
        assert!(!options.security.is_encrypted());
    }

    #[test]
    fn debug_redacts_the_password() {
        let rendered = format!("{:?}", Credentials::user_name("collector", "hunter2"));
        assert!(rendered.contains("collector"));
        assert!(!rendered.contains("hunter2"));
    }

    #[test]
    fn credentials_without_encryption_are_refused() {
        let options = ConnectOptions::default()
            .security(Security::Sign(SecurityPolicy::Basic256Sha256))
            .user_name("collector", "hunter2");

        assert!(matches!(
            connect("opc.tcp://127.0.0.1:1", &options),
            Err(Error::InsecureCredentials)
        ));
    }

    #[test]
    fn security_maps_to_wire_modes() {
        assert_eq!(Security::None.mode(), MessageSecurityMode::None);
        assert_eq!(
            Security::Sign(SecurityPolicy::Basic256Sha256).mode(),
            MessageSecurityMode::Sign
        );
        assert_eq!(
            Security::SignAndEncrypt(SecurityPolicy::Basic256Sha256).policy(),
            SecurityPolicy::Basic256Sha256
        );
    }
}
