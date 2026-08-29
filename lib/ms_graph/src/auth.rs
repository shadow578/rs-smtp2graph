use crate::config::Config;
use anyhow::{Result, anyhow};
use log::{debug, trace};
use serde::Deserialize;
use std::time::Instant;

/// response to OAuth2 token grant request.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AccessToken {
    /// the requested access token.
    access_token: String,

    /// type of the access token. always "Bearer".
    token_type: String,

    /// how long the access token is valid, in seconds from token issue.
    expires_in: u64,

    /// time the token was issued at, set by AccessToken::get().
    #[serde(skip)]
    issued_at: Option<Instant>,
}

impl AccessToken
{
    /// create a new token instance from fixed values.
    /// note: only for use in unit tests.
    #[cfg(test)]
    pub(crate) fn new_for_test(access_token: &str, token_type: &str, expires_in: u64, issued_at: Instant) -> Self {
        Self {
            access_token: access_token.into(),
            token_type: token_type.into(),
            expires_in,
            issued_at: Some(issued_at),
        }
    }

    /// get OAuth2 token for client using client_credentials grant.
    /// config: client config.
    pub(crate) async fn get_client_credentials(config: &Config) -> Result<Self>
    {
        let url = format!("{}/{}/oauth2/v2.0/token", config.login_endpoint(), config.tenant_id());
        trace!("auth with url {}", url);

        let mut token = config.http_client()
            .post(&url)
            .form(&[
                ("client_id", config.client_id()),
                ("client_secret", config.client_secret()),
                ("scope", "https://graph.microsoft.com/.default"),
                ("grant_type", "client_credentials"),
            ])
            .send()
            .await
            .and_then(|res| res.error_for_status())
            .map_err(|e| e.without_url())?
            .json::<Self>()
            .await?;

        token.issued_at = Some(Instant::now());

        // expect bearer token only
        if token.token_type != "Bearer" {
            return Err(anyhow!("Invalid token type '{}'", token.token_type));
        }

        debug!("Got OAuth2 access token for tenant_id={} client_id={}", config.tenant_id(), config.client_id());
        Ok(token)
    }

    /// check if this token is expired (via expires_in)
    pub(crate) fn is_expired(&self) -> bool {
        if let Some(issued_at) = self.issued_at {
            issued_at.elapsed().as_secs() > self.expires_in
        } else {
            // don't know when issued, assume not expired
            false
        }
    }

    /// get the access token.
    pub(crate) fn access_token(&self) -> &str
    {
        &self.access_token
    }
}

#[cfg(test)]
mod tests
{
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_token_expiration()
    {
        let token = AccessToken {
            access_token: "mock-access-token".into(),
            token_type: "Bearer".into(),
            expires_in: 3599,
            issued_at: Some(Instant::now() - Duration::from_secs(3600)),
        };

        assert!(token.is_expired());
    }
}
