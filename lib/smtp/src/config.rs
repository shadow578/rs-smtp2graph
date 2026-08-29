use crate::handler::Handler;
use crate::tls_config::TlsConfig;
use crate::{AuthMode, DEFAULT_MAX_MESSAGE_SIZE};
use anyhow::Result;
use tokio_rustls::TlsAcceptor;

/// SMTP server and session configuration.
#[derive(Clone)]
pub struct Config<H>
where
    H: Handler,
{
    /// listen address for the server.
    /// e.g. 0.0.0.0:2525.
    address: String,

    /// TLS acceptor for StartTLS, if configured.
    /// if not configured, StartTLS will not be available.
    tls: Option<TlsAcceptor>,

    /// configuration for smtp sessions created by the server.
    session_config: SessionConfig<H>,
}

impl<H> Config<H>
where
    H: Handler,
{
    /// create a new configuration object for the given event handler.
    /// handler: mandatory SMTP session event handler.
    pub fn new(handler: H) -> Self {
        Self {
            address: "0.0.0.0:25".into(),
            tls: None,
            session_config: SessionConfig {
                handler,
                server_name: "localhost".into(),
                auth_mode: AuthMode::None,
                max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
            },
        }
    }

    /// set listen address for the SMTP server.
    /// by default, "0.0.0.0:25" is used.
    /// address: listen address.
    pub fn with_address<T>(&mut self, address: T) -> &mut Self
    where
        T: Into<String>,
    {
        self.address = address.into();
        self
    }

    /// set server name of SMTP server.
    /// by default, "localhost" is used.
    /// server_name: name of SMTP server.
    pub fn with_server_name<T>(&mut self, server_name: T) -> &mut Self
    where
        T: Into<String>,
    {
        self.session_config.server_name = server_name.into();
        self
    }

    /// set authentication mode.
    /// by default, AuthMode::None is used.
    /// auth_mode: authentication mode.
    pub fn with_auth(&mut self, auth_mode: AuthMode) -> &mut Self
    {
        self.session_config.auth_mode = auth_mode;
        self
    }

    /// set maximum message size.
    /// by default, DEFAULT_MAX_MESSAGE_SIZE is used.
    /// max_message_size: maximum message size.
    pub fn with_max_message_size(&mut self, max_message_size: usize) -> &mut Self {
        self.session_config.max_message_size = max_message_size;
        self
    }

    /// set TLS configuration.
    /// by default, no TLS is configured.
    /// tls_config: TLS configuration object.
    pub fn with_tls(&mut self, tls_config: TlsConfig) -> Result<&mut Self>
    {
        self.tls = tls_config.into_tls_acceptor()?.into();
        Ok(self)
    }

    /// get server listen address.
    pub(crate) fn address(&self) -> &String
    {
        &self.address
    }

    /// get TLS acceptor instance (cloned).
    pub(crate) fn tls(&self) -> Option<TlsAcceptor>
    {
        self.tls.clone()
    }

    /// get session config (cloned).
    pub(crate) fn session_config(&self) -> SessionConfig<H>
    {
        self.session_config.clone()
    }
}

/// SMTP session configuration.
#[derive(Clone)]
pub(crate) struct SessionConfig<H>
where
    H: Handler,
{
    /// SMTP session event handler.
    handler: H,

    /// name of this SMTP server.
    server_name: String,

    /// authentication handling mode.
    auth_mode: AuthMode,

    /// maximum size of received mail object.
    max_message_size: usize,
}

impl<H> SessionConfig<H>
where
    H: Handler,
{
    /// get SMTP session event handler.
    pub(crate) fn handler(&mut self) -> &mut H
    {
        &mut self.handler
    }

    /// get server name.
    pub(crate) fn server_name(&self) -> &String
    {
        &self.server_name
    }

    /// get authentication mode.
    pub(crate) fn auth_mode(&self) -> AuthMode
    {
        self.auth_mode
    }

    /// get maximum message size
    pub(crate) fn max_message_size(&self) -> usize
    {
        self.max_message_size
    }

    /// does this configuration configure authentication in any way?
    pub(crate) fn has_auth(&self) -> bool
    {
        self.auth_mode != AuthMode::None
    }
}
