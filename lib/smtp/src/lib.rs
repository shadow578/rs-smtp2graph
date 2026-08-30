use std::time::Duration;

pub mod server;
mod connection;
mod session;
pub mod handler;
mod response;
mod command;
pub mod config;
pub mod tls_config;

#[cfg(test)]
mod session_tests;

/// authentication modes for SMTP session
#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum AuthMode
{
    /// don't require any authentication.
    None,

    /// require authentication, allow always.
    /// validate credentials in your handlers on_login().
    Always,

    /// require authentication, allow only after StartTls.
    /// validate credentials in your handlers on_login().
    RequireTls,
}

/// SMTP Mail object
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct Mail
{
    /// mail sender, as per envelope (MAIL FROM:<from>).
    from: String,

    /// mail recipients, as per envelope (MAIL TO:<to>).
    to: Vec<String>,

    /// mail MIME data, unprocessed.
    data: Vec<u8>,
}

impl Mail
{
    /// construct an empty mail object.
    pub(crate) fn empty() -> Self
    {
        Mail { from: String::new(), to: Vec::new(), data: Vec::new() }
    }

    /// set the sender of this mail, overriding existing.
    /// from: sender address to set.
    pub(crate) fn set_sender(&mut self, from: String)
    {
        self.from = Self::normalize_address(from);
    }

    /// add a recipient to this mail.
    /// to: recipient address to add.
    pub(crate) fn add_recipient(&mut self, to: String)
    {
        self.to.push(Self::normalize_address(to));
    }

    /// append mail MIME data to this mail's data buffer.
    /// data: mail data to append.
    pub(crate) fn append_data(&mut self, data: &[u8])
    {
        self.data.extend_from_slice(data);
    }

    /// get the sender address of this mail.
    pub fn sender(&self) -> &str
    {
        &self.from
    }

    /// get the recipient addresses of this mail.
    pub fn recipients(&self) -> &[String]
    {
        &self.to
    }

    /// get the raw, unprocessed MIME mail data buffer.
    pub fn data(&self) -> &[u8]
    {
        &self.data
    }

    /// get the length of the data buffer returned by Mail::data().
    pub fn data_length(&self) -> usize
    {
        self.data.len()
    }

    /// normalize mail address.
    /// e.g. <bob@example.com> => bob@example.com
    /// address: the address to normalize
    fn normalize_address(address: String) -> String
    {
        let address = address.trim();
        let address = address.strip_prefix("<").unwrap_or(address);
        let address = address.strip_suffix(">").unwrap_or(address);
        address.trim().into()
    }
}

/// Default maximum message size.
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

/// timeout for Session::read_line().
const SESSION_READ_LINE_TIMEOUT: Duration = Duration::from_secs(30);

/// timeout for Session::reply().
const SESSION_REPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// maximum duration a SMTP session may last.
const SESSION_MAX_DURATION: Duration = Duration::from_mins(5);
