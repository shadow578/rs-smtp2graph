mod mock {
    use crate::config::Config;
    use crate::connection::SmtpClientConnection;
    use crate::handler::Handler;
    use crate::session::Session;
    use async_trait::async_trait;
    use std::io;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, duplex};
    use tokio::task::JoinHandle;
    use tokio::time::timeout;
    use tokio_rustls::TlsAcceptor;

    const MOCK_CLIENT_TIMEOUT: Duration = Duration::from_secs(5);


    // region: connection mocking
    pub(super) struct MockClientConnection {
        tx: DuplexStream,
        rx: DuplexStream,
        is_tls: Arc<AtomicBool>,
    }

    #[async_trait]
    impl SmtpClientConnection for MockClientConnection
    {
        async fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
            self.tx.write_all(data).await
        }

        async fn flush(&mut self) -> io::Result<()> {
            self.tx.flush().await
        }

        async fn read_byte(&mut self) -> io::Result<Option<u8>> {
            let mut byte = [0u8; 1];
            let n = self.rx.read(&mut byte).await?;
            if n == 0 {
                Ok(None)
            } else {
                Ok(Some(byte[0]))
            }
        }

        async fn start_tls(self: Box<Self>, _tls: &TlsAcceptor) -> io::Result<Box<dyn SmtpClientConnection>> {
            self.is_tls.store(true, Ordering::SeqCst);

            Ok(Box::from(MockClientConnection {
                rx: self.rx,
                tx: self.tx,
                is_tls: self.is_tls,
            }))
        }

        fn is_tls(&self) -> bool {
            self.is_tls.load(Ordering::SeqCst)
        }
    }

    pub(super) struct MockClient
    {
        tx: DuplexStream,
        rx: BufReader<DuplexStream>,
        is_tls: Arc<AtomicBool>,
    }

    impl MockClient
    {
        pub(super) async fn write_line(&mut self, line: &str) -> io::Result<()> {
            timeout(MOCK_CLIENT_TIMEOUT, async {
                self.tx.write_all(line.as_bytes()).await?;
                self.tx.write_all(b"\r\n").await?;
                self.tx.flush().await
            }).await?
        }

        pub(super) async fn expect_lines(&mut self, expected: &[&str]) -> anyhow::Result<()>
        {
            for line in expected {
                self.expect_line(line).await?;
            }
            Ok(())
        }

        pub(super) async fn expect_line(&mut self, expected: &str) -> anyhow::Result<()>
        {
            let received = timeout(MOCK_CLIENT_TIMEOUT, self.read_line()).await??;
            assert_eq!(received, expected);
            Ok(())
        }

        async fn read_line(&mut self) -> anyhow::Result<String>
        {
            let mut buf = Vec::new();
            self.rx.read_until(b'\n', &mut buf).await?;

            if buf.ends_with(b"\r\n") {
                buf.truncate(buf.len() - 2);
            }
            if buf.ends_with(b"\n") {
                buf.truncate(buf.len() - 1);
            }

            Ok(String::from_utf8(buf)?)
        }

        pub(super) fn is_tls(&self) -> bool {
            self.is_tls.load(Ordering::SeqCst)
        }
    }

    fn make_connection_pair() -> (MockClient, MockClientConnection)
    {
        let (client_tx, server_rx) = duplex(1024);
        let (server_tx, client_rx) = duplex(1024);
        let is_tls = Arc::new(AtomicBool::new(false));

        (MockClient { tx: client_tx, rx: BufReader::new(client_rx), is_tls: is_tls.clone() }, MockClientConnection { tx: server_tx, rx: server_rx, is_tls })
    }

    // endregion

    // region: handler mocking
    #[derive(Clone)]
    pub(super) struct MockHandler {}

    impl MockHandler
    {
        pub(super) fn new() -> Self
        {
            MockHandler {}
        }
    }

    #[async_trait]
    impl Handler for MockHandler {}

    // endregion

    // region: session creation
    pub(super) async fn create_mocked_session<F>(config_fn: F) -> (MockClient, MockHandler, JoinHandle<anyhow::Result<()>>)
    where
        F: FnOnce(&mut Config<MockHandler>),
    {
        let (client, client_connection) = make_connection_pair();
        let handler = MockHandler::new();
        let handler_for_session = handler.clone();

        let mut config = Config::new(handler_for_session);
        config_fn(&mut config);

        let session_handle = tokio::spawn(async move {
            let mut session = Session::new(Box::new(client_connection), &mut config);
            session.handle().await
        });

        (client, handler, session_handle)
    }

    // endregion
}

#[tokio::test]
async fn test_banner() -> anyhow::Result<()> {
    let (mut client, _handler, handle) = mock::create_mocked_session(|c| {
        c.with_server_name("mocked_server");
    }).await;

    client.expect_line("220 2.2.0 mocked_server ESMTP ready").await?;

    handle.abort();
    Ok(())
}

#[tokio::test]
async fn test_greeting() -> anyhow::Result<()> {
    pretty_env_logger::init();

    let (mut client, _handler, handle) = mock::create_mocked_session(|c| {
        c.with_server_name("mocked_server");
    }).await;

    client.expect_line("220 2.2.0 mocked_server ESMTP ready").await?;

    client.write_line("HELO mocked_client").await?;

    client.expect_line("250 mocked_server").await?;

    handle.abort();
    Ok(())
}

