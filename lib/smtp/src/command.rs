use crate::command::Command::{Mail, Recipient};

/// SMTP parsed commands.
#[derive(Debug)]
pub(crate) enum Command
{
    /// client hello
    /// HELO <domain>
    /// EHLO <domain>
    Hello { domain: String, extended: bool },

    /// help request
    /// HELP
    Help,

    /// upgrade to TLS connection request
    /// STARTTLS
    StartTls,

    /// authenticate session
    /// AUTH <method> <initial>
    Authenticate { method: AuthMethod, initial: Option<Vec<u8>> },

    /// start mail object
    /// MAIL FROM:<from>
    Mail { from: String },

    /// define mail recipient
    /// RCPT TO:<to>
    Recipient { to: String },

    /// beginning of mail data
    /// DATA
    Data,

    /// abort current transaction
    /// RSET
    Reset,

    /// no operation
    /// NOOP
    NoOp,

    /// close connection
    /// QUIT
    Quit,
}

/// available modes for SMTP AUTH command.
#[derive(Debug)]
pub(crate) enum AuthMethod
{
    /// AUTH PLAIN
    Plain,

    /// AUTH LOGIN
    Login,
}

impl Command
{
    /// parse SMTP command from given input line.
    /// CRLF is stripped, if present.
    pub(crate) fn parse(input: &[u8]) -> Result<Self, &'static str>
    {
        let input = input.strip_suffix(b"\r\n")
            .or_else(|| input.strip_suffix(b"\n"))
            .unwrap_or(input);

        let (verb, arg) = split_none_or_once(input, b' ');

        match verb.to_ascii_uppercase().as_slice() {
            b"HELO" => {
                let domain = arg.ok_or("HELO requires domain")?;
                let domain = String::from_utf8_lossy(domain).trim().to_owned();
                Ok(Command::Hello { domain, extended: false })
            }
            b"EHLO" => {
                let domain_raw = arg.ok_or("HELO requires domain")?;
                let domain = String::from_utf8_lossy(domain_raw).trim().to_owned();
                Ok(Command::Hello { domain, extended: true })
            }
            b"HELP" => Ok(Command::Help),
            b"STARTTLS" => {
                if arg.is_some() {
                    Err("STARTTLS takes no arguments")
                } else {
                    Ok(Command::StartTls)
                }
            }
            b"AUTH" => {
                let arg = arg.ok_or("AUTH requires mechanism")?;
                let (mechanism, initial) = split_none_or_once(arg, b' ');
                let initial = initial.map(|initial| initial.to_vec());

                match mechanism.to_ascii_uppercase().as_slice() {
                    b"PLAIN" => {
                        Ok(
                            Command::Authenticate {
                                method: AuthMethod::Plain,
                                initial,
                            }
                        )
                    }
                    b"LOGIN" => {
                        Ok(
                            Command::Authenticate {
                                method: AuthMethod::Login,
                                initial,
                            }
                        )
                    }
                    _ => Err("unsupported AUTH mechanism")
                }
            }
            b"MAIL" => {
                let from = arg.ok_or("MAIL requires FROM")?;
                let from = strip_prefix_case_insensitive(from, b"FROM:").ok_or("MAIL requires FROM")?;
                let from = String::from_utf8_lossy(from).trim().to_owned();
                Ok(Mail { from })
            }
            b"RCPT" => {
                let to = arg.ok_or("RCPT requires TO")?;
                let to = strip_prefix_case_insensitive(to, b"TO:").ok_or("RCPT requires TO")?;
                let to = String::from_utf8_lossy(to).trim().to_owned();
                Ok(Recipient { to })
            }
            b"DATA" => if arg.is_some() { Err("DATA takes no arguments") } else { Ok(Command::Data) }
            b"RSET" => if arg.is_some() { Err("RSET takes no arguments") } else { Ok(Command::Reset) }
            b"NOOP" => if arg.is_some() { Err("NOOP takes no arguments") } else { Ok(Command::NoOp) }
            b"QUIT" => if arg.is_some() { Err("QUIT takes no arguments") } else { Ok(Command::Quit) }
            _ => Err("unrecognized command"),
        }
    }
}

/// strip prefix from input data, if present.
/// input: input data to strip.
/// prefix: prefix to strip from input.
fn strip_prefix_case_insensitive<'a>(
    input: &'a [u8],
    prefix: &[u8],
) -> Option<&'a [u8]> {
    if input.len() < prefix.len() {
        return None;
    }

    let matches =
        input[..prefix.len()]
            .iter()
            .zip(prefix)
            .all(|(a, b)| a.eq_ignore_ascii_case(b));

    matches.then(|| &input[prefix.len()..])
}

/// split input at delimiter once, or - if delimiter not found, return as-is.
/// e.g. input="foo-bar" delim="-" => ("foo", Some("bar")).
/// e.g. input="foo" delim"-" => ("foo", None).
/// input: input to split.
/// delimiter: delimiter to split on.
fn split_none_or_once(input: &[u8], delimiter: u8) -> (&[u8], Option<&[u8]>)
{
    match input.iter().position(|&x| x == delimiter) {
        Some(pos) => (
            &input[..pos],
            Some(&input[pos + 1..])
        ),
        None => (input, None)
    }
}
