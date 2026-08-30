use crate::command::{AuthMethod, Command};
use crate::config::SessionConfig;
use crate::connection::SmtpClientConnection;
use crate::handler::{Handler, HelloResult, LoginResult};
use crate::response::{AUTH_CHALLENGE_LOGIN_PASSWORD, AUTH_CHALLENGE_LOGIN_USERNAME, AUTH_CHALLENGE_PLAIN, AUTH_FAIL, AUTH_OK, AUTH_REQUIRED, BAD_SEQUENCE, DATA_START, DATA_TOO_LONG, GOODBYE, MAIL_ACCEPTED, MAIL_HANDLER_ERROR, OK, RESET, Response, START_TLS};
use crate::{AuthMode, Mail, SESSION_READ_LINE_TIMEOUT, SESSION_REPLY_TIMEOUT};
use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use log::{debug, error, trace};
use memchr::memchr;
use std::io;
use tokio::time::timeout;

/// maximum length of a single SMTP line, including CRLF.
/// RFC 5321 §4.5.3.1.6 (https://datatracker.ietf.org/doc/html/rfc5321#section-4.5.3.1.6)
/// sets this at 1000 octets, we allow a bit more
const MAX_LINE_LENGTH: usize = 1500;

/// result type for Session::handle_auth
#[derive(Debug)]
enum AuthHandshakeResult
{
    /// auth successfully.
    /// refer to fields for credentials.
    Ok { username: String, password: String },

    /// syntax error during auth handling.
    /// variant contains error description.
    SyntaxError(String),
}

/// result type for Session::handle_data
#[derive(Debug)]
enum HandleDataResult
{
    /// more data to capture
    Continue,

    /// data capture completed
    DataEnd,

    /// message too long; drop connection
    DataTooLong,
}

/// phases of SMTP session.
#[derive(Debug, PartialEq)]
enum Phase
{
    /// initial state before HELO
    /// or after transaction completed (DATA end)
    Connected,

    /// after HELO until MAIL FROM
    Greeted,

    /// after MAIL FROM, during (multiple) RCPT TO, until DATA
    Envelope,

    /// after DATA, during capture, until "."
    Data,
}

/// SMTP session state structure
#[derive(Debug, PartialEq)]
struct SessionState
{
    /// what phase is this session in?
    phase: Phase,

    /// has the client successfully authenticated?
    is_authenticated: bool,

    /// mail object currently being captured, if any.
    current_mail: Mail,
}

impl SessionState
{
    /// construct a new session state reflecting the state
    /// at the start of a session (or after reset).
    fn new() -> SessionState
    {
        SessionState {
            phase: Phase::Connected,
            is_authenticated: false,
            current_mail: Mail::empty(),
        }
    }

    /// is this session in the specified phase?
    /// phase: the phase to check for
    fn in_phase(&self, phase: Phase) -> bool
    {
        self.phase == phase
    }
}

/// SMTP session keeping and handling
pub(crate) struct Session<'a, H: Handler>
where
    H: Handler,
{
    /// the connection to the client.
    /// may be secured via TLS.
    connection: Option<Box<dyn SmtpClientConnection>>,

    /// configuration to use for this session, including event handler.
    config: &'a mut SessionConfig<H>,

    /// the state of the session.
    state: SessionState,
}

impl<'a, H> Session<'a, H>
where
    H: Handler,
{
    /// construct a new session for an existing connection.
    /// connection: the client connection.
    /// config: configuration for the session, including event handler.
    pub(crate) fn new(connection: Box<dyn SmtpClientConnection>, config: &'a mut SessionConfig<H>) -> Self
    {
        Session {
            connection: Some(connection),
            config,
            state: SessionState::new(),
        }
    }

    /// handle the SMTP connection to the client.
    /// returns once the client disconnects, or the connection should be dropped.
    pub(crate) async fn handle(&mut self) -> Result<()>
    {
        self.reply(Response::banner(self.config.server_name())).await?;

        loop
        {
            let line = match self.read_line().await? {
                Some(line) => line,
                None => return Err(anyhow!("unexpected EOF")), // closes connection
            };

            if self.state.in_phase(Phase::Data) {
                match self.handle_data(line).await?
                {
                    HandleDataResult::Continue => (),
                    HandleDataResult::DataEnd => {
                        if let Err(err) = self.config.handler().on_mail(&self.state.current_mail).await
                        {
                            error!("during handler.on_mail: {err}");
                            self.reply(MAIL_HANDLER_ERROR.extend(err.to_string())).await?;
                        } else {
                            self.reply(MAIL_ACCEPTED).await?;
                        }

                        // reset only mail data, state remains to accept potential second mail
                        self.state.current_mail = Mail::empty();
                        self.state.phase = Phase::Greeted;
                    }
                    HandleDataResult::DataTooLong => {
                        return Err(anyhow!("message data was too long.")); // closes connection
                    }
                }
                continue;
            }

            let command = match Command::parse(line.as_slice()) {
                Ok(command) => command,
                Err(err) => {
                    self.reply(Response::syntax_error(Some(err))).await?;
                    continue;
                }
            };

            match command {
                Command::Hello { domain, extended } => {
                    if !self.state.in_phase(Phase::Connected)
                    {
                        self.reply(BAD_SEQUENCE).await?;
                        continue;
                    }

                    match self.config.handler().on_hello(&domain, extended)
                        .await
                        .unwrap_or_else(|err| {
                            error!("during handler.on_hello: {err}");
                            HelloResult::Reject
                        })
                    {
                        HelloResult::Ok => (),
                        HelloResult::Reject => {
                            return Err(anyhow!("client rejected during hello")); // closes connection
                        }
                    }

                    if extended {
                        self.reply(Response::new_continued(250, self.config.server_name().clone())).await?;

                        let mut features: Vec<String> = vec!["8BITMIME".into()];

                        if self.supports_tls()? {
                            features.push("STARTTLS".into());
                        }
                        if self.config.has_auth() {
                            features.push("AUTH PLAIN LOGIN".into());
                        }

                        features.push(format!("SIZE {}", self.config.max_message_size()));

                        for (i, feat) in features.iter().enumerate() {
                            let response = if i == features.len() - 1 {
                                Response::new(250, feat.to_string())
                            } else {
                                Response::new_continued(250, feat.to_string())
                            };

                            self.reply(response).await?;
                        }
                    } else {
                        self.reply(Response::new(250, self.config.server_name().clone())).await?;
                    }

                    self.state.phase = Phase::Greeted;
                }
                Command::Help => {
                    self.reply(Response::new_continued(250, "Commands supported")).await?;

                    // list STARTTLS and AUTH only if enabled
                    let mut commands: Vec<&str> = vec!["HELO", "EHLO", "HELP", "MAIL", "RCPT", "DATA", "RSET", "NOOP", "QUIT"];
                    if self.config.has_auth() {
                        commands.push("AUTH");
                    }
                    if self.supports_tls()? {
                        commands.push("STARTTLS");
                    }

                    self.reply(Response::new(214, commands.join(" "))).await?;
                }
                Command::StartTls => {
                    if !self.supports_tls()? {
                        self.reply(Response::syntax_error("TLS not supported".into())).await?;
                        continue;
                    }

                    if !self.state.in_phase(Phase::Greeted) {
                        self.reply(BAD_SEQUENCE).await?;
                        continue;
                    }

                    self.reply(START_TLS).await?;

                    debug!("Upgrading connection to TLS");
                    let connection = self.connection.take().unwrap();
                    self.connection = Some(connection.start_tls().await?);

                    // reset state after TLS upgrade
                    self.state = SessionState::new();
                }
                Command::Authenticate { method, initial } => {
                    if !self.state.in_phase(Phase::Greeted) {
                        self.reply(BAD_SEQUENCE).await?;
                        continue;
                    }

                    match self.config.auth_mode() {
                        AuthMode::None => {
                            self.reply(Response::syntax_error(None)).await?;
                            continue;
                        }
                        AuthMode::RequireTls => {
                            let tls = self.connection.as_ref()
                                .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "no connection"))?
                                .is_tls();

                            if !tls {
                                self.reply(BAD_SEQUENCE).await?;
                                continue;
                            }
                        }
                        AuthMode::Always => {}
                    }

                    match self.handle_auth(method, initial).await? {
                        AuthHandshakeResult::Ok { username, password } => {
                            match self.config.handler().on_login(username, password)
                                .await
                                .unwrap_or_else(|err| {
                                    error!("during handler.on_login: {err}");
                                    LoginResult::Reject
                                })
                            {
                                LoginResult::Ok => {
                                    self.reply(AUTH_OK).await?;
                                    self.state.is_authenticated = true;
                                }
                                LoginResult::Reject => {
                                    self.reply(AUTH_FAIL).await?;
                                    return Err(anyhow!("client rejected during login")); // closes connection
                                }
                            }
                        }
                        AuthHandshakeResult::SyntaxError(err) => {
                            self.reply(Response::syntax_error(Some(&*err))).await?;
                        }
                    }
                }
                Command::Mail { from } => {
                    if !self.state.in_phase(Phase::Greeted)
                    {
                        self.reply(BAD_SEQUENCE).await?;
                        continue;
                    }

                    if !self.is_authenticated()
                    {
                        self.reply(AUTH_REQUIRED).await?;
                        continue;
                    }

                    self.state.current_mail = Mail::empty();
                    self.state.current_mail.set_sender(from);

                    self.state.phase = Phase::Envelope;

                    self.reply(OK).await?;
                }
                Command::Recipient { to } => {
                    if !self.state.in_phase(Phase::Envelope)
                    {
                        self.reply(BAD_SEQUENCE).await?;
                        continue;
                    }

                    if !self.is_authenticated()
                    {
                        self.reply(AUTH_REQUIRED).await?;
                        continue;
                    }

                    self.state.current_mail.add_recipient(to);

                    self.reply(OK).await?;
                }
                Command::Data => {
                    if !self.state.in_phase(Phase::Envelope)
                    {
                        self.reply(BAD_SEQUENCE).await?;
                        continue;
                    }

                    if !self.is_authenticated()
                    {
                        self.reply(AUTH_REQUIRED).await?;
                        continue;
                    }

                    self.state.phase = Phase::Data;

                    self.reply(DATA_START).await?;
                }
                Command::Reset => {
                    self.state = SessionState::new();
                    self.reply(RESET).await?;

                    if let Err(err) = self.config.handler().on_reset().await
                    {
                        error!("during handler.on_reset: {err}");
                    }
                }
                Command::NoOp => {
                    self.reply(OK).await?;
                }
                Command::Quit => {
                    self.reply(GOODBYE).await?;
                    return Ok(()); // close connection
                }
            }
        }
    }

    /// handle authentication handshake with the client
    /// method: method specified by the client in AUTH command.
    /// initial: payload specified in AUTH command, if any. e.g. AUTH PLAIN <initial>.
    async fn handle_auth(&mut self, method: AuthMethod, initial: Option<Vec<u8>>) -> Result<AuthHandshakeResult>
    {
        match method {
            AuthMethod::Plain => {
                let credentials = match initial {
                    Some(c) => c,
                    None => {
                        self.reply(AUTH_CHALLENGE_PLAIN).await?;

                        let Some(ln) = self.read_line().await?
                        else {
                            return Ok(AuthHandshakeResult::SyntaxError("unexpected EOF in AUTH PLAIN challenge".into()));
                        };
                        ln
                    }
                };

                let credentials = match Self::decode_base64(&credentials) {
                    Ok(c) => c,
                    Err(_) => {
                        return Ok(AuthHandshakeResult::SyntaxError("invalid base64 data".into()));
                    }
                };

                match credentials.split(|&x| x == 0).collect::<Vec<&[u8]>>().as_slice()
                {
                    [_, username, password] => {
                        let username = String::from_utf8_lossy(username).to_string();
                        let password = String::from_utf8_lossy(password).to_string();

                        Ok(AuthHandshakeResult::Ok { username, password })
                    }
                    _ => {
                        Ok(AuthHandshakeResult::SyntaxError("invalid number of parts in PLAIN credential payload".into()))
                    }
                }
            }
            AuthMethod::Login => {
                // username via initial (AUTH LOGIN <username>)
                // or via challenge
                let username = match initial {
                    Some(usr) => usr,
                    None => {
                        self.reply(AUTH_CHALLENGE_LOGIN_USERNAME).await?;
                        let Some(ln) = self.read_line().await?
                        else {
                            return Ok(AuthHandshakeResult::SyntaxError("unexpected EOF in AUTH LOGIN challenge".into()));
                        };
                        ln
                    }
                };
                let username = Self::decode_base64(&username)?;
                let username = String::from_utf8_lossy(&username).to_string();

                // password always via challenge
                self.reply(AUTH_CHALLENGE_LOGIN_PASSWORD).await?;
                let Some(password) = self.read_line().await?
                else {
                    return Ok(AuthHandshakeResult::SyntaxError("unexpected EOF in AUTH LOGIN challenge".into()));
                };
                let password = Self::decode_base64(&password)?;
                let password = String::from_utf8_lossy(&password).to_string();

                Ok(AuthHandshakeResult::Ok { username, password })
            }
        }
    }

    /// handle mail data capture from the client (in Data phase).
    /// line: the line received from the client.
    async fn handle_data(&mut self, line: Vec<u8>) -> Result<HandleDataResult>
    {
        // end of data?
        if line == b".\r\n" || line == b".\n" {
            return Ok(HandleDataResult::DataEnd);
        }

        // dot transparency to allow dots on lines
        // without accidentally triggering data end
        // "..\r\n" -> ".\r\n"
        let line = if line.starts_with(b"..") { &line[1..] } else { &line };

        if self.state.current_mail.data_length() + line.len() > self.config.max_message_size()
        {
            self.reply(DATA_TOO_LONG).await?;
            return Ok(HandleDataResult::DataTooLong);
        }

        self.state.current_mail.append_data(line);

        Ok(HandleDataResult::Continue)
    }

    /// is the client authenticated, if required?
    fn is_authenticated(&self) -> bool
    {
        if self.config.has_auth() { self.state.is_authenticated } else { true }
    }

    /// does the underlying connection support upgrading to TLS?
    fn supports_tls(&self) -> Result<bool>
    {
        Ok(
            self.connection.as_ref()
                .ok_or_else(|| anyhow!("connection was not valid"))?
                .supports_tls()
        )
    }

    /// get the connection instance.
    fn get_connection(&mut self) -> Result<&mut Box<dyn SmtpClientConnection>>
    {
        self.connection.as_mut()
            .ok_or_else(|| anyhow!("connection was not valid"))
    }

    /// decode base64 data
    /// input: the base64 encoded input data
    fn decode_base64(input: &[u8]) -> Result<Vec<u8>>
    {
        let input = input.strip_suffix(b"\r\n")
            .or_else(|| input.strip_suffix(b"\n"))
            .unwrap_or(input)
            .trim_ascii();

        BASE64.decode(input)
            .with_context(|| anyhow!("base64 decode error"))
    }

    /// read a line from the client, up until CRLF, handling timeout. result includes CRLF.
    async fn read_line(&mut self) -> Result<Option<Vec<u8>>>
    {
        let line = timeout(SESSION_READ_LINE_TIMEOUT, async {
            let connection = self.get_connection()?;
            let mut line: Vec<u8> = Vec::with_capacity(MAX_LINE_LENGTH);

            loop {
                // grab buffer from connection, as many bytes as available
                let buf = connection.fill_buf().await?;
                if buf.is_empty() {
                    return if line.is_empty() {
                        Ok(None)
                    } else {
                        Err(anyhow!("unexpected EOF in SMTP line read"))
                    };
                }

                // search for newline
                // append all up until newline, or whole buffer if no newline
                let n = memchr(b'\n', buf)
                    .map(|n| n + 1)
                    .unwrap_or(buf.len());
                if line.len() + n > MAX_LINE_LENGTH {
                    return Err(anyhow!("SMTP line is too long"));
                }

                line.extend_from_slice(&buf[..n]);
                connection.consume(n).await;

                // check if line complete
                if line.last() == Some(&b'\n') {
                    return Ok(Some(line));
                }
            }
        }).await??;

        if let Some(ln) = line.as_ref() {
            trace!("SMTP recv> {}", String::from_utf8_lossy(ln).trim_end_matches(['\r', '\n']));
        }

        Ok(line)
    }

    /// send a reply to the client, applying timeout.
    /// response: the reply to send.
    async fn reply(&mut self, response: Response) -> Result<()>
    {
        timeout(SESSION_REPLY_TIMEOUT, async {
            let line = response.line();
            trace!("SMTP send: {line}");

            let mut line = line.as_bytes().to_vec();
            line.extend_from_slice(b"\r\n");

            let connection = self.get_connection()?;
            connection.write_all(line.as_slice()).await?;
            connection.flush().await?;
            Ok(())
        }).await?
    }
}
