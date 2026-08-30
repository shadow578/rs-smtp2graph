use crate::AuthMode;
use crate::session_tests::mock::MockHandlerCallbackRecord::{Hello, Login, Mail};
use rstest::rstest;
use std::time::Duration;
use tokio::time::timeout;

mod mock {
    use crate::Mail;
    use crate::config::Config;
    use crate::connection::SmtpClientConnection;
    use crate::handler::{Handler, HelloResult, LoginResult};
    use crate::session::Session;
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::error::Error;
    use std::io;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, duplex};
    use tokio::sync::Mutex;
    use tokio::task::JoinHandle;
    use tokio::time::timeout;

    const MOCK_CLIENT_TIMEOUT: Duration = Duration::from_secs(2);

    // region: connection mocking
    pub(super) struct MockClientConnection {
        tx: DuplexStream,
        rx: DuplexStream,
        supports_tls: bool,
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

        async fn start_tls(self: Box<Self>) -> io::Result<Box<dyn SmtpClientConnection>> {
            self.is_tls.store(true, Ordering::SeqCst);

            Ok(Box::from(MockClientConnection {
                rx: self.rx,
                tx: self.tx,
                supports_tls: false,
                is_tls: self.is_tls,
            }))
        }

        fn supports_tls(&self) -> bool {
            self.supports_tls
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


        pub(super) async fn expect_line(&mut self, expected: &str) -> anyhow::Result<()>
        {
            let received = self.line().await?;
            assert_eq!(received, expected);
            Ok(())
        }

        pub(super) async fn skip_line(&mut self) -> anyhow::Result<()> {
            let _ = self.line().await?;
            Ok(())
        }

        pub(super) async fn line(&mut self) -> anyhow::Result<String> {
            timeout(MOCK_CLIENT_TIMEOUT, self.read_line()).await?
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

    fn make_connection_pair(supports_tls: bool) -> (MockClient, MockClientConnection)
    {
        let (client_tx, server_rx) = duplex(1024);
        let (server_tx, client_rx) = duplex(1024);
        let is_tls = Arc::new(AtomicBool::new(false));

        (MockClient { tx: client_tx, rx: BufReader::new(client_rx), is_tls: is_tls.clone() }, MockClientConnection { tx: server_tx, rx: server_rx, is_tls, supports_tls })
    }
    // endregion

    // region: handler mocking
    #[derive(Debug, Eq, PartialEq)]
    pub(super) enum MockHandlerCallbackRecord {
        Hello { domain: String, extended: bool },
        Login { username: String, password: String },
        Mail { mail: Mail },
        Reset,
    }

    #[derive(Clone)]
    pub(super) struct MockHandler {
        callbacks: Arc<Mutex<VecDeque<MockHandlerCallbackRecord>>>,
    }

    impl MockHandler
    {
        fn new() -> Self
        {
            MockHandler {
                callbacks: Arc::new(Mutex::new(VecDeque::new())),
            }
        }

        pub(super) async fn pop_and_expect(&mut self, expected: MockHandlerCallbackRecord) {
            assert_eq!(self.pop().await, Some(expected));
        }

        pub(super) async fn pop_and_expect_none(&mut self) {
            assert_eq!(self.pop().await, None);
        }

        pub(super) async fn pop_all_and_ignore(&mut self) {
            self.callbacks.lock().await.clear();
        }

        pub(super) async fn pop(&mut self) -> Option<MockHandlerCallbackRecord> {
            self.callbacks.lock().await.pop_front()
        }

        async fn push(&mut self, record: MockHandlerCallbackRecord) {
            self.callbacks.lock().await.push_back(record)
        }
    }

    #[async_trait]
    impl Handler for MockHandler {
        async fn on_hello(&mut self, domain: &str, extended: bool) -> Result<HelloResult, Box<dyn Error + Send + Sync>> {
            self.push(MockHandlerCallbackRecord::Hello { domain: domain.into(), extended }).await;
            Ok(HelloResult::Ok)
        }

        async fn on_login(&mut self, username: String, password: String) -> Result<LoginResult, Box<dyn Error + Send + Sync>> {
            self.push(MockHandlerCallbackRecord::Login { username, password }).await;
            Ok(LoginResult::Ok)
        }

        async fn on_mail(&mut self, mail: &Mail) -> Result<(), Box<dyn Error + Send + Sync>> {
            self.push(MockHandlerCallbackRecord::Mail { mail: mail.clone() }).await;
            Ok(())
        }

        async fn on_reset(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
            self.push(MockHandlerCallbackRecord::Reset).await;
            Ok(())
        }
    }

    // endregion

    // region: session creation
    pub(super) async fn create_mocked_session<F>(with_tls: bool, config_fn: F) -> (MockClient, MockHandler, JoinHandle<anyhow::Result<()>>)
    where
        F: FnOnce(&mut Config<MockHandler>),
    {
        let (client, client_connection) = make_connection_pair(with_tls);
        let handler = MockHandler::new();
        let handler_for_session = handler.clone();

        let mut config = Config::new(handler_for_session);
        config_fn(&mut config);

        let mut config = config.session_config();

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
    let (mut client, _handler, handle) = mock::create_mocked_session(false, |c| {
        c.with_server_name("mocked_server");
    }).await;

    client.expect_line("220 2.2.0 mocked_server ESMTP ready").await?;

    handle.abort();
    Ok(())
}

#[tokio::test]
async fn test_hello_basic() -> anyhow::Result<()> {
    let (mut client, mut handler, handle) = mock::create_mocked_session(false, |c| {
        c.with_server_name("mocked_server");
    }).await;

    client.expect_line("220 2.2.0 mocked_server ESMTP ready").await?;

    // client greets
    client.write_line("HELO mocked_client").await?;

    // server replies
    client.expect_line("250 mocked_server").await?;

    // handler was called with details of HELO
    handler.pop_and_expect(Hello { domain: "mocked_client".into(), extended: false }).await;


    handle.abort();
    Ok(())
}

#[rstest]
#[case::no_tls(false)]
#[case::with_tls(true)]
#[tokio::test]
async fn test_hello_extended(#[case] with_tls: bool) -> anyhow::Result<()> {
    let (mut client, mut handler, handle) = mock::create_mocked_session(with_tls, |c| {
        c.with_server_name("mocked_server")
            .with_auth(AuthMode::Always)
            .with_max_message_size(1024);
    }).await;

    client.expect_line("220 2.2.0 mocked_server ESMTP ready").await?;

    // EHLO
    client.write_line("EHLO mocked_client").await?;
    client.expect_line("250-mocked_server").await?; // capabilities follow

    // read capability response lines back, stripping prefix.
    let mut capabilities = Vec::new();
    while let Ok(line) = client.line().await
    {
        let (line, end) = if line.starts_with("250-") {
            (line.trim_start_matches("250-"), false)
        } else if line.starts_with("250 ") {
            (line.trim_start_matches("250 "), true)
        } else {
            panic!("EHLO capabilities expect 250 command code");
        };

        capabilities.push(String::from(line));
        if end {
            break;
        }
    }

    let mut expected_capabilities: Vec<String> = vec!["8BITMIME".into(), "AUTH PLAIN LOGIN".into(), "SIZE 1024".into()];
    if with_tls {
        expected_capabilities.push("STARTTLS".into());
    }

    capabilities.sort();
    expected_capabilities.sort();
    assert_eq!(capabilities, expected_capabilities);

    // handler was called
    handler.pop_and_expect(Hello { domain: "mocked_client".into(), extended: true }).await;

    handle.abort();
    Ok(())
}

#[rstest]
#[case::no_tls(false)]
#[case::with_tls(true)]
#[tokio::test]
async fn test_help(#[case] with_tls: bool) -> anyhow::Result<()> {
    let (mut client, _handler, handle) = mock::create_mocked_session(with_tls, |c| {
        c.with_server_name("mocked_server")
            .with_auth(AuthMode::Always)
            .with_max_message_size(1024);
    }).await;

    client.skip_line().await?;

    client.write_line("HELP").await?;
    client.expect_line("250-Commands supported").await?;

    let mut reported_commands = client.line().await?
        .split(" ")
        .map(String::from)
        .collect::<Vec<_>>();

    let mut expected_commands: Vec<String> = vec![
        "214".into(), // response code, listed here to keep parsing simple
        "HELO".into(),
        "EHLO".into(),
        "HELP".into(),
        "AUTH".into(),
        "MAIL".into(),
        "RCPT".into(),
        "DATA".into(),
        "RSET".into(),
        "NOOP".into(),
        "QUIT".into()
    ];
    if with_tls {
        expected_commands.push("STARTTLS".into());
    }

    reported_commands.sort();
    expected_commands.sort();
    assert_eq!(reported_commands, expected_commands);

    handle.abort();
    Ok(())
}

#[tokio::test]
async fn test_reset() -> anyhow::Result<()> {
    let (mut client, _handler, handle) = mock::create_mocked_session(false, |c| {
        c.with_server_name("mocked_server");
    }).await;

    client.skip_line().await?;

    // first greeting
    client.write_line("HELO mocked_client").await?;
    client.skip_line().await?;

    // reset session, resetting back to non-greeted state
    client.write_line("RSET").await?;
    client.expect_line("250 2.0.0 RESET").await?;

    // MAIL fails because greeting is missing
    client.write_line("MAIL FROM:alice@example.com").await?;
    client.expect_line("503 5.5.1 Bad sequence of commands").await?;

    handle.abort();
    Ok(())
}

#[tokio::test]
async fn test_quit() -> anyhow::Result<()> {
    let (mut client, _handler, handle) = mock::create_mocked_session(false, |c| {
        c.with_server_name("mocked_server");
    }).await;

    client.skip_line().await?;

    client.write_line("HELO mocked_client").await?;
    client.skip_line().await?;

    client.write_line("QUIT").await?;
    client.expect_line("221 2.0.0 Goodbye").await?;

    // session should close, and handle should exit soon after
    timeout(Duration::from_secs(2), handle).await???;
    Ok(())
}

#[tokio::test]
async fn test_start_tls() -> anyhow::Result<()> {
    let (mut client, mut handler, handle) = mock::create_mocked_session(true, |c| {
        c.with_server_name("mocked_server");
    }).await;

    client.skip_line().await?;

    client.write_line("HELO mocked_client").await?;
    client.skip_line().await?;
    handler.pop_all_and_ignore().await;

    // negotiate tls
    client.write_line("STARTTLS").await?;
    client.expect_line("220 2.0.0 Ready to start TLS").await?;

    // we're now on a tls connection
    assert!(client.is_tls());

    handle.abort();
    Ok(())
}

#[tokio::test]
async fn test_auth_plain() -> anyhow::Result<()> {
    let (mut client, mut handler, handle) = mock::create_mocked_session(false, |c| {
        c.with_server_name("mocked_server")
            .with_auth(AuthMode::Always);
    }).await;

    client.skip_line().await?;

    client.write_line("HELO mocked_client").await?;
    client.skip_line().await?;
    handler.pop_all_and_ignore().await;

    // AUTH PLAIN inline
    client.write_line("AUTH PLAIN AGFsaWNlAGh1bnRlcjI=").await?;
    client.expect_line("235 2.7.0 Authentication succeeded").await?;
    handler.pop_and_expect(Login { username: "alice".into(), password: "hunter2".into() }).await;

    handle.abort();
    Ok(())
}

#[tokio::test]
async fn test_auth_plain_challenge() -> anyhow::Result<()> {
    let (mut client, mut handler, handle) = mock::create_mocked_session(false, |c| {
        c.with_server_name("mocked_server")
            .with_auth(AuthMode::Always);
    }).await;

    client.skip_line().await?;

    client.write_line("HELO mocked_client").await?;
    client.skip_line().await?;
    handler.pop_all_and_ignore().await;

    // AUTH PLAIN /w challenge-response
    client.write_line("AUTH PLAIN").await?;
    client.expect_line("334").await?;

    client.write_line("AGFsaWNlAGh1bnRlcjI=").await?;
    client.expect_line("235 2.7.0 Authentication succeeded").await?;
    handler.pop_and_expect(Login { username: "alice".into(), password: "hunter2".into() }).await;

    handle.abort();
    Ok(())
}

#[tokio::test]
async fn test_auth_login_inline() -> anyhow::Result<()> {
    let (mut client, mut handler, handle) = mock::create_mocked_session(false, |c| {
        c.with_server_name("mocked_server")
            .with_auth(AuthMode::Always);
    }).await;

    client.skip_line().await?;

    client.write_line("HELO mocked_client").await?;
    client.skip_line().await?;
    handler.pop_all_and_ignore().await;

    // AUTH LOGIN /w username in-line
    client.write_line("AUTH LOGIN YWxpY2U=").await?;
    client.expect_line("334 UGFzc3dvcmQ6").await?;
    client.write_line("aHVudGVyMg==").await?;
    client.expect_line("235 2.7.0 Authentication succeeded").await?;
    handler.pop_and_expect(Login { username: "alice".into(), password: "hunter2".into() }).await;

    handle.abort();
    Ok(())
}

#[tokio::test]
async fn test_auth_login_challenge() -> anyhow::Result<()> {
    let (mut client, mut handler, handle) = mock::create_mocked_session(false, |c| {
        c.with_server_name("mocked_server")
            .with_auth(AuthMode::Always);
    }).await;

    client.skip_line().await?;

    client.write_line("HELO mocked_client").await?;
    client.skip_line().await?;
    handler.pop_all_and_ignore().await;

    // AUTH LOGIN /w both as challenge-response
    client.write_line("AUTH LOGIN").await?;
    client.expect_line("334 VXNlcm5hbWU6").await?;
    client.write_line("YWxpY2U=").await?;
    client.expect_line("334 UGFzc3dvcmQ6").await?;
    client.write_line("aHVudGVyMg==").await?;
    client.expect_line("235 2.7.0 Authentication succeeded").await?;
    handler.pop_and_expect(Login { username: "alice".into(), password: "hunter2".into() }).await;

    handle.abort();
    Ok(())
}

#[tokio::test]
async fn test_auth_require_tls() -> anyhow::Result<()> {
    let (mut client, mut handler, handle) = mock::create_mocked_session(true, |c| {
        c.with_server_name("mocked_server")
            .with_auth(AuthMode::RequireTls);
    }).await;

    client.skip_line().await?;

    client.write_line("HELO mocked_client").await?;
    client.skip_line().await?;
    handler.pop_all_and_ignore().await;

    client.write_line("STARTTLS").await?;
    client.skip_line().await?;

    // we should be tls enabled now
    assert!(client.is_tls());

    // greet again after STARTTLS, as state is reset on upgrade
    client.write_line("HELO mocked_client").await?;
    client.skip_line().await?;
    handler.pop_all_and_ignore().await;

    // AUTH PLAIN works, we started tls above
    client.write_line("AUTH PLAIN AGFsaWNlAGh1bnRlcjI=").await?;
    client.expect_line("235 2.7.0 Authentication succeeded").await?;
    handler.pop_and_expect(Login { username: "alice".into(), password: "hunter2".into() }).await;

    handle.abort();
    Ok(())
}
#[tokio::test]
async fn test_auth_require_tls_fail_no_tls() -> anyhow::Result<()> {
    let (mut client, mut handler, handle) = mock::create_mocked_session(true, |c| {
        c.with_server_name("mocked_server")
            .with_auth(AuthMode::RequireTls);
    }).await;

    client.skip_line().await?;

    client.write_line("HELO mocked_client").await?;
    client.skip_line().await?;
    handler.pop_all_and_ignore().await;

    // AUTH PLAIN fails due to missing TLS
    client.write_line("AUTH PLAIN AGFsaWNlAGh1bnRlcjI=").await?;
    client.expect_line("503 5.5.1 Bad sequence of commands").await?;
    handler.pop_and_expect_none().await;

    handle.abort();
    Ok(())
}

#[tokio::test]
async fn test_full_transaction_basic() -> anyhow::Result<()> {
    let (mut client, mut handler, handle) = mock::create_mocked_session(false, |c| {
        c.with_server_name("mocked_server")
            .with_auth(AuthMode::Always);
    }).await;

    // banner
    client.expect_line("220 2.2.0 mocked_server ESMTP ready").await?;

    // HELO:
    client.write_line("HELO mocked_client").await?;
    client.expect_line("250 mocked_server").await?;
    handler.pop_and_expect(Hello { domain: "mocked_client".into(), extended: false }).await;

    // AUTH:
    client.write_line("AUTH PLAIN AGFsaWNlAGh1bnRlcjI=").await?;
    client.expect_line("235 2.7.0 Authentication succeeded").await?;
    handler.pop_and_expect(Login { username: "alice".into(), password: "hunter2".into() }).await;

    // Mail:
    client.write_line("MAIL FROM:alice@example.com").await?;
    client.expect_line("250 2.0.0 OK").await?;

    client.write_line("RCPT TO:bob@example.com").await?;
    client.expect_line("250 2.0.0 OK").await?;

    client.write_line("RCPT TO:eve@example.com").await?;
    client.expect_line("250 2.0.0 OK").await?;

    client.write_line("DATA").await?;
    client.expect_line("354 3.0.0 Start mail input; end with <CRLF>.<CRLF>").await?;

    const MIME_DATA: &str = "From: alice@example.com\r\nTo: bob@example.com\r\nSubject: Test\r\n\r\nHello Bob.\r\n";

    for line in MIME_DATA.lines() {
        client.write_line(line).await?;
    }
    client.write_line(".").await?;
    client.expect_line("250 2.0.0 Mail accepted").await?;

    if let Some(Mail { mail }) = handler.pop().await {
        assert_eq!(mail.sender(), "alice@example.com");
        assert_eq!(mail.recipients(), vec!["bob@example.com", "eve@example.com"]);
        assert_eq!(mail.data(), MIME_DATA.as_bytes().to_vec());
    } else {
        panic!("expected Mail handler to have been called");
    }

    // only one call to on_mail to pop, nothing more
    handler.pop_and_expect_none().await;

    // quit
    client.write_line("QUIT").await?;
    client.expect_line("221 2.0.0 Goodbye").await?;

    // no more calls, on_connect and on_disconnect are not part of session
    handler.pop_and_expect_none().await;

    handle.abort();
    Ok(())
}
