use crate::Mail;
use async_trait::async_trait;
use std::error::Error;
use std::net::IpAddr;

/// result for Handler::on_connect
pub enum ConnectResult
{
    /// Allow connection normally.
    Ok,

    /// Reject this client, drop connection.
    Reject,
}

/// result for Handler::on_hello.
#[derive(Debug)]
pub enum HelloResult
{
    /// Hello ok, accept connection.
    Ok,

    /// Reject this client, drop connection.
    Reject,
}


/// result for Handler::on_login.
#[derive(Debug)]
pub enum LoginResult
{
    /// Login successfully, accept client.
    Ok,

    /// Login failed, reject client.
    Reject,
}

/// SMTP server event handler.
/// note: this handler is cloned for each new session.
#[allow(async_fn_in_trait, unused_variables)]
#[async_trait]
pub trait Handler: Clone + Send + Sync + 'static
{
    /// called on client connect / session start.
    /// peer_addr: IP of remote peer.
    async fn on_connect(&mut self, peer_addr: IpAddr) -> Result<ConnectResult, Box<dyn Error + Send + Sync>>
    {
        Ok(ConnectResult::Ok)
    }

    /// called on client disconnect / session end.
    /// note that the reason for the disconnect is not specified for this function.
    /// disconnect could have happened due to an error, because authentication failed, or
    /// simply after a successful session completed.
    /// peer_addr: IP of remote peer.
    async fn on_disconnect(&mut self, peer_addr: IpAddr) -> Result<(), Box<dyn Error + Send + Sync>>
    {
        Ok(())
    }

    /// called on HELO / EHLO received.
    /// domain: domain specified in HELO / EHLO command
    /// extended: is this extended HELO (EHLO)?
    async fn on_hello(&mut self, domain: &str, extended: bool) -> Result<HelloResult, Box<dyn Error + Send + Sync>>
    {
        Ok(HelloResult::Ok)
    }

    /// called on AUTH PLAIN / AUTH LOGIN received.
    /// validate credentials here.
    /// username: username supplied by client
    /// password: password supplied by client
    async fn on_login(&mut self, username: String, password: String) -> Result<LoginResult, Box<dyn Error + Send + Sync>>
    {
        Ok(LoginResult::Ok)
    }

    /// called after mail is fully received
    /// mail: Mail object
    async fn on_mail(&mut self, mail: &Mail) -> Result<(), Box<dyn Error + Send + Sync>>
    {
        Ok(())
    }

    /// called on transaction reset (RSET)
    /// reset any internal state you've modified here, if applicable
    async fn on_reset(&mut self) -> Result<(), Box<dyn Error + Send + Sync>>
    {
        Ok(())
    }
}
