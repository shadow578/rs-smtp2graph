use crate::connection::Connection::{Plain, Tls};
use async_trait::async_trait;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
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

    /// read all data from read buffer, reading more if that buffer is empty.
    /// note: this does *not* consume the bytes. use consume() for that.
    async fn fill_buf(&mut self) -> io::Result<&[u8]>;

    /// consume len bytes from the read data buffer.
    async fn consume(&mut self, len: usize);

    /// upgrade this connection to TLS (SMTP StartTLS).
    /// note: only valid when supports_tls() is true.
    async fn start_tls(self: Box<Self>) -> io::Result<Box<dyn SmtpClientConnection>>;

    /// does this connection support upgrading to TLS via start_tls?
    /// note: this returns false for connections that are already tls.
    fn supports_tls(&self) -> bool;

    /// is this a TLS connection?
    fn is_tls(&self) -> bool;
}

/// helper to abstract plain and TLS streams into common interface.
pub(crate) enum Connection {
    /// plain TCP connection.
    Plain(BufReader<TcpStream>, Option<TlsAcceptor>),

    /// TLS secured TCP connection.
    Tls(Box<BufReader<TlsStream<PrefixedTcpStream>>>),
}

#[async_trait]
impl SmtpClientConnection for Connection {
    async fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        match self {
            Plain(stream, _) => stream.write_all(data).await,
            Tls(stream) => stream.write_all(data).await,
        }
    }

    async fn flush(&mut self) -> io::Result<()> {
        match self {
            Plain(stream, _) => stream.flush().await,
            Tls(stream) => stream.flush().await,
        }
    }

    async fn fill_buf(&mut self) -> io::Result<&[u8]> {
        match self {
            Plain(stream, _) => stream.fill_buf().await,
            Tls(stream) => stream.fill_buf().await,
        }
    }

    async fn consume(&mut self, len: usize) {
        match self {
            Plain(stream, _) => stream.consume(len),
            Tls(stream) => stream.consume(len),
        };
    }

    async fn start_tls(self: Box<Connection>) -> io::Result<Box<dyn SmtpClientConnection>> {
        match *self {
            Plain(stream, tls) =>
                {
                    let tls = tls.ok_or(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "connection cannot support TLS",
                    ))?;

                    let stream = PrefixedTcpStream::from_existing(stream);
                    let stream = tls.accept(stream).await?;
                    Ok(Box::from(Connection::new_tls(stream)))
                }
            Tls(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "connection is already TLS",
            )),
        }
    }

    fn supports_tls(&self) -> bool {
        match self {
            Plain(_, tls) => tls.is_some(),
            Tls(_) => false,
        }
    }

    fn is_tls(&self) -> bool {
        match self {
            Plain(_, _) => false,
            Tls(_) => true,
        }
    }
}

impl Connection {
    /// create a new connection from a plain TCP stream.
    /// stream: TCP stream from/to the client.
    /// tls: (optional) tls acceptor for StartTLS.
    pub(crate) fn new_plain(stream: TcpStream, tls: Option<TlsAcceptor>) -> Self {
        Plain(BufReader::new(stream), tls)
    }

    /// create a new connection from TLS stream.
    /// stream: TLS stream from/to the client.
    fn new_tls(stream: TlsStream<PrefixedTcpStream>) -> Self {
        Tls(BufReader::new(stream).into())
    }
}


/// TCP stream wrapper that replays bytes read ahead first.
pub(crate) struct PrefixedTcpStream {
    prefix: Vec<u8>,
    inner: TcpStream,
}

impl PrefixedTcpStream {
    /// create a new prefixed stream, using the already buffered bytes in the reader as prefix.
    /// reader: BufReader instance to use.
    fn from_existing(reader: BufReader<TcpStream>) -> Self {
        Self {
            prefix: reader.buffer().to_vec(),
            inner: reader.into_inner(),
        }
    }
}

impl AsyncRead for PrefixedTcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.prefix.is_empty() {
            let n = self.prefix.len().min(buf.remaining());
            buf.put_slice(&self.prefix[..n]);
            self.prefix.drain(..n);
            return Poll::Ready(Ok(()));
        }

        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for PrefixedTcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
