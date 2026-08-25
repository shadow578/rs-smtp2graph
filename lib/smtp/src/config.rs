use crate::handler::Handler;
use crate::tls_config::TlsConfig;
use crate::{AuthMode, DEFAULT_MAX_MESSAGE_SIZE};
use anyhow::Result;
use tokio_rustls::TlsAcceptor;

/// SMTP server and session configuration.
#[derive(Clone)]
pub struct Config<H: Handler>
where
    H: Handler,
{
    /// SMTP session event handler.
    handler: H,

    /// listen address for the server.
    /// e.g. 0.0.0.0:2525.
    address: String,

    /// name of this SMTP server.
    server_name: String,

    /// authentication handling mode.
    auth_mode: AuthMode,

    /// maximum size of received mail object.
    max_message_size: usize,

    /// TLS acceptor for StartTLS, if configured.
    /// if not configured, StartTLS will not be available.
    tls: Option<TlsAcceptor>,
}
impl<H> Config<H>
where
    H: Handler,
{
    /// create a new configuration object for the given event handler.
    /// handler: mandatory SMTP session event handler.
    pub fn new(handler: H) -> Self {
        Self {
            handler,
            address: "0.0.0.0:25".into(),
            server_name: "localhost".into(),
            auth_mode: AuthMode::None,
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
            tls: None,
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
        self.server_name = server_name.into();
        self
    }

    /// set authentication mode.
    /// by default, AuthMode::None is used.
    /// auth_mode: authentication mode.
    pub fn with_auth(&mut self, auth_mode: AuthMode) -> &mut Self
    {
        self.auth_mode = auth_mode;
        self
    }

    /// set maximum message size.
    /// by default, DEFAULT_MAX_MESSAGE_SIZE is used.
    /// max_message_size: maximum message size.
    pub fn with_max_message_size(&mut self, max_message_size: usize) -> &mut Self {
        self.max_message_size = max_message_size;
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

    /// get SMTP session event handler.
    pub(crate) fn handler(&mut self) -> &mut H
    {
        &mut self.handler
    }

    /// get server listen address.
    pub(crate) fn address(&self) -> &String
    {
        &self.address
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

    /// get TLS acceptor instance.
    pub(crate) fn tls(&self) -> Option<&TlsAcceptor>
    {
        self.tls.as_ref()
    }

    /// does this configuration configure authentication in any way?
    pub(crate) fn has_auth(&self) -> bool
    {
        self.auth_mode != AuthMode::None
    }

    /// does this configuration configure TLS?
    pub(crate) fn has_tls(&self) -> bool
    {
        self.tls.is_some()
    }
}