use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

/// helper to abstract plain and TLS streams into common interface.
#[derive(Debug)]
pub(crate) enum Connection {
    /// plain TCP connection.
    Plain(TcpStream),

    /// TLS secured TCP connection.
    Tls(Box<TlsStream<TcpStream>>),
}

impl Connection {
    /// write data to the connection.
    /// data: data to write to the connection.
    pub(crate) async fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        match self {
            Self::Plain(stream) => stream.write_all(data).await,
            Self::Tls(stream) => stream.write_all(data).await,
        }
    }

    /// flush the connection's write buffer.
    pub(crate) async fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush().await,
            Self::Tls(stream) => stream.flush().await,
        }
    }

    /// read a single byte from the connection.
    pub(crate) async fn read_byte(&mut self) -> io::Result<Option<u8>> {
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

    /// upgrade this connection to TLS (SMTP StartTLS).
    /// tls: TLS acceptor instance to use for upgrade.
    pub(crate) async fn start_tls(self, tls: &TlsAcceptor) -> io::Result<Self> {
        match self {
            Self::Plain(stream) =>
                {
                    let tls_stream = tls.accept(stream).await?;
                    Ok(Connection::Tls(tls_stream.into()))
                }
            Self::Tls(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "connection is already TLS",
            )),
        }
    }

    /// is this a TLS connection?
    pub(crate) fn is_tls(&self) -> bool {
        match self {
            Self::Plain(_) => false,
            Self::Tls(_) => true,
        }
    }
}
