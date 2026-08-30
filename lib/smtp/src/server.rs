use crate::SESSION_MAX_DURATION;
use crate::config::Config;
use crate::connection::Connection;
use crate::handler::{ConnectResult, Handler};
use crate::session::Session;
use anyhow::Result;
use log::{debug, error, info, warn};
use tokio::net::TcpListener;
use tokio::time::timeout;

/// simple SMTP server implementation
#[derive(Debug)]
pub struct Server {}

impl Server
{
    /// start listening for SMTP clients using the provided configuration details.
    /// config: SMTP server configuration, including handler
    pub async fn listen<H>(config: &Config<H>) -> Result<()>
    where
        H: Handler,
    {
        let listener = TcpListener::bind(config.address()).await
            .unwrap_or_else(|_| panic!("Failed to bind to {}", config.address()));

        info!("SMTP server listening on {}", listener.local_addr()?);

        loop {
            let (stream, peer_addr) = listener.accept().await?;

            let connection = Connection::new_plain(stream, config.tls());
            let mut session_config = config.session_config();

            tokio::spawn(async move {
                debug!("Accepted connection from {peer_addr}");

                match session_config.handler().on_connect(peer_addr.ip()).await
                {
                    Ok(ConnectResult::Ok) => {
                        let mut session = Session::new(Box::new(connection), &mut session_config);
                        match timeout(SESSION_MAX_DURATION, session.handle()).await
                        {
                            Ok(Ok(())) => {}
                            Ok(Err(err)) => {
                                warn!("Connection with {} aborted with error: {}", peer_addr, err);
                            }
                            Err(_) => {
                                warn!("Connection with {} exceeded maximum session duration after {} seconds", peer_addr, SESSION_MAX_DURATION.as_secs());
                            }
                        }

                        debug!("Connection with {peer_addr} closed");
                    }
                    Ok(ConnectResult::Reject) => {
                        warn!("Connection from {peer_addr} rejected due to handler verdict.");
                    }
                    Err(err) => {
                        error!("during handler.on_connect: {err}");
                    }
                }

                if let Err(err) = session_config.handler().on_disconnect(peer_addr.ip()).await
                {
                    error!("during handler.on_connect: {err}");
                }
            });
        }
    }
}

