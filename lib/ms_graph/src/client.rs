use crate::auth::AccessToken;
use crate::config::Config;
use anyhow::{Result, anyhow};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use log::{debug, trace};

/// M365 graph api mail client.
#[derive(Debug, Clone)]
pub struct Client
{
    /// client configuration.
    config: Config,

    /// access token, if authenticated.
    token: Option<AccessToken>,
}

impl Client
{
    /// construct a new graph client instance.
    pub fn new(config: Config) -> Self
    {
        Self {
            config,
            token: None,
        }
    }

    /// authenticate the graph client, if required.
    pub async fn authenticate(&mut self) -> Result<bool>
    {
        if !self.is_authenticated()
        {
            debug!("Acquiring new access token");
            self.token = AccessToken::get_client_credentials(&self.config).await?
                .into();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// is this client authenticated?
    fn is_authenticated(&self) -> bool
    {
        self.token.is_some() && !self.token.as_ref().unwrap().is_expired()
    }

    /// send mail using ms graph API's sendMail
    /// https://learn.microsoft.com/en-us/graph/api/user-sendmail
    /// sender: user id or upn of the sending user
    /// mail_mime_data: raw mail mime data to be sent.
    pub async fn send_mail(&mut self, sender: &str, mail_mime_data: &[u8]) -> Result<()>
    {
        self.authenticate().await?;
        if !self.is_authenticated()
        {
            // this probably can't happen, i think...
            return Err(anyhow!("Failed to acquire auth token"));
        }


        let url = format!("{}/users/{}/sendMail", self.config.graph_endpoint(), sender);
        trace!("sendMail with url {}", url);

        let mail_data = BASE64.encode(mail_mime_data);

        debug!("Sending mail w/ {} bytes ({} b64 encoded) as user {}", mail_mime_data.len(), mail_data.len(), sender);

        self.config.http_client()
            .post(&url)
            .bearer_auth(self.token.as_ref().unwrap().access_token())
            .header("Content-Type", "text/plain")
            .body(mail_data)
            .send()
            .await
            .and_then(|res| res.error_for_status())
            .map_err(|e| e.without_url())?;

        Ok(())
    }
}
