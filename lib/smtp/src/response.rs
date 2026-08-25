/// Ready to start TLS.
pub(crate) const START_TLS: Response = Response::fixed(220, "2.0.0 Ready to start TLS");

/// Goodbye.
pub(crate) const GOODBYE: Response = Response::fixed(221, "2.0.0 Goodbye");

/// Authentication succeeded.
pub(crate) const AUTH_OK: Response = Response::fixed(235, "2.7.0 Authentication succeeded");

/// OK.
pub(crate) const OK: Response = Response::fixed(250, "2.0.0 OK");

/// RESET.
pub(crate) const RESET: Response = Response::fixed(250, "2.0.0 RESET");

/// Mail accepted.
pub(crate) const MAIL_ACCEPTED: Response = Response::fixed(250, "2.0.0 Mail accepted");

/// AUTH PLAIN challenge.
pub(crate) const AUTH_CHALLENGE_PLAIN: Response = Response::fixed(334, "");

/// AUTH LOGIN Username: challenge.
pub(crate) const AUTH_CHALLENGE_LOGIN_USERNAME: Response = Response::fixed(334, /* Username: */ "VXNlcm5hbWU6");

/// AUTH LOGIN Password: challenge.
pub(crate) const AUTH_CHALLENGE_LOGIN_PASSWORD: Response = Response::fixed(334, /* Password: */ "UGFzc3dvcmQ6");

/// Start mail input; end with <CRLF>.<CRLF>.
pub(crate) const DATA_START: Response = Response::fixed(354, "3.0.0 Start mail input; end with <CRLF>.<CRLF>");

/// Internal error during mail processing.
pub(crate) const MAIL_HANDLER_ERROR: Response = Response::fixed(451, "4.3.0 Error during mail processing");

/// Bad sequence of commands.
pub(crate) const BAD_SEQUENCE: Response = Response::fixed(503, "5.5.1 Bad sequence of commands");

/// Authentication required.
pub(crate) const AUTH_REQUIRED: Response = Response::fixed(530, "5.7.0 Authentication required");

/// Authentication failed.
pub(crate) const AUTH_FAIL: Response = Response::fixed(535, "5.7.8 Authentication failed");

/// Mail size exceeds fixed maximum message size.
pub(crate) const DATA_TOO_LONG: Response = Response::fixed(552, "5.3.4 Mail size exceeds fixed maximum message size");

/// helper to handle static and dynamic strings for response construction.
#[derive(Clone, Debug)]
enum Message
{
    Fixed(&'static str),
    Custom(String),
}

/// SMTP response.
#[derive(Clone, Debug)]
pub(crate) struct Response {
    /// SMTP status code.
    code: u16,

    /// message string.
    message: Message,

    /// is this a continued response ("-" between code and message)?
    has_next: bool,
}

impl Response {
    /// construct a constant response.
    /// code: SMTP status code
    /// message: response message string
    const fn fixed(code: u16, message: &'static str) -> Self
    {
        Response {
            code,
            message: Message::Fixed(message),
            has_next: false,
        }
    }

    /// construct a 220 banner response.
    /// server_name: server name to report in banner, e.g. service name or hostname.
    pub(crate) fn banner<T>(server_name: T) -> Self
    where
        T: Into<String>,
    {
        Response::new(220, format!("2.2.0 {} ESMTP ready", server_name.into()))
    }

    /// construct a 500 syntax error message.
    /// err: syntax error description string, if any.
    pub(crate) fn syntax_error(err: Option<&str>) -> Self
    {
        Response::new(
            500,
            if let Some(err) = err
            { format!("5.5.2 Syntax error: {}", err) } else { "5.5.2 Syntax error".into() })
    }

    /// construct a new response with custom message.
    /// code: SMTP status code.
    /// message: custom message string.
    pub(crate) fn new<T>(code: u16, message: T) -> Self
    where
        T: Into<String>,
    {
        Response {
            code,
            message: Message::Custom(message.into()),
            has_next: false,
        }
    }

    /// construct a new response that is continued by another response.
    /// e.g.: for EHLO.
    /// code: SMTP status code.
    /// message: custom message string.
    pub(crate) fn new_continued<T>(code: u16, message: T) -> Self
    where
        T: Into<String>,
    {
        Response {
            code,
            message: Message::Custom(message.into()),
            has_next: true,
        }
    }

    /// extend the message of the existing response by an extension.
    /// resulting response object has the same code, but the message is extended.
    /// e.g. OK.extend_message("foo") -> "2.0.0 OK: foo".
    /// extension: string to add to the message string.
    pub fn extend<T>(&self, extension: T) -> Self
    where
        T: Into<String>,
    {
        let message = format!("{}: {}", self.message(), extension.into());
        Self {
            code: self.code,
            message: Message::Custom(message),
            has_next: self.has_next,
        }
    }

    /// unwrap message string.
    fn message(&self) -> &str {
        match &self.message {
            Message::Custom(message) => message,
            Message::Fixed(message) => message,
        }
    }

    /// get line data for this response.
    /// does NOT include CRLF.
    pub(crate) fn line(&self) -> String
    {
        format!("{}{}{}",
                self.code,
                if self.has_next { "-" } else { " " },
                self.message()
        )
    }
}
