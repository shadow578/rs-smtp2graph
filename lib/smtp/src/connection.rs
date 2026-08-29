use async_trait::async_trait;
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

/// trait for a connection to an SMTP client.
#[allow(async_fn_in_trait, unused_variables)]
#[async_trait]
pub(crate) trait SmtpClientConnection: Send + Sync + 'static
{
    /// write data to the connection.
    /// data: data to write to the connection.
    async fn write_all(&mut self, data: &[u8]) -> io::Result<()>;

    /// flush the connection's write buffer.
    async fn flush(&mut self) -> io::Result<()>;

    /// read a single byte from the connection.
    async fn read_byte(&mut self) -> io::Result<Option<u8>>;

    /// upgrade this connection to TLS (SMTP StartTLS).
    /// tls: TLS acceptor instance to use for upgrade.
    async fn start_tls(self: Box<Self>, tls: &TlsAcceptor) -> io::Result<Box<dyn SmtpClientConnection>>;

    /// is this a TLS connection?
    fn is_tls(&self) -> bool;
}

/// helper to abstract plain and TLS streams into common interface.
#[derive(Debug)]
pub(crate) enum Connection {
    /// plain TCP connection.
    Plain(TcpStream),

    /// TLS secured TCP connection.
    Tls(Box<TlsStream<TcpStream>>),
}

#[async_trait]
impl SmtpClientConnection for Connection {
    async fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        match self {
            Self::Plain(stream) => stream.write_all(data).await,
            Self::Tls(stream) => stream.write_all(data).await,
        }
    }

    async fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush().await,
            Self::Tls(stream) => stream.flush().await,
        }
    }

    async fn read_byte(&mut self) -> io::Result<Option<u8>> {
        let mut byte = [0u8; 1];

        let n = match self {
            Self::Plain(stream) => stream.read(&mut byte).await?,
            Self::Tls(stream) => stream.read(&mut byte).await?,
        };

        if n == 0 {
            Ok(None)
        } else {
            Ok(Some(byte[0]))
        }
    }
    async fn start_tls(self: Box<Connection>, tls: &TlsAcceptor) -> io::Result<Box<dyn SmtpClientConnection>> {
        match *self {
            Self::Plain(stream) =>
                {
                    let tls_stream = tls.accept(stream).await?;
                    Ok(Box::from(Connection::Tls(tls_stream.into())))
                }
            Self::Tls(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "connection is already TLS",
            )),
        }
    }

    fn is_tls(&self) -> bool {
        match self {
            Self::Plain(_) => false,
            Self::Tls(_) => true,
        }
    }
}
