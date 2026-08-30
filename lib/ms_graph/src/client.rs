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
        if self.get_authentication().is_err()
        {
            debug!("Acquiring new access token");
            self.token = AccessToken::get_client_credentials(&self.config).await?
                .into();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// get the authentication token of the client.
    /// returns Ok(AccessToken) when authenticated, and Err when not.
    fn get_authentication(&self) -> Result<&AccessToken>
    {
        if let Some(token) = self.token.as_ref() && !token.is_expired() {
            Ok(token)
        } else {
            Err(anyhow!("not authenticated or expired"))
        }
    }

    /// send mail using ms graph API's sendMail
    /// https://learn.microsoft.com/en-us/graph/api/user-sendmail
    /// sender: user id or upn of the sending user
    /// mail_mime_data: raw mail mime data to be sent.
    pub async fn send_mail(&mut self, sender: &str, mail_mime_data: &[u8]) -> Result<()>
    {
        self.authenticate().await?;

        let url = format!("{}/users/{}/sendMail", self.config.graph_endpoint(), sender);
        trace!("sendMail with url {}", url);

        let mail_data = BASE64.encode(mail_mime_data);

        debug!("Sending mail w/ {} bytes ({} b64 encoded) as user {}", mail_mime_data.len(), mail_data.len(), sender);

        self.config.http_client()
            .post(&url)
            .bearer_auth(self.get_authentication()?.access_token())
            .header("Content-Type", "text/plain")
            .body(mail_data)
            .send()
            .await
            .and_then(|res| res.error_for_status())
            .map_err(|e| e.without_url())?;

        Ok(())
    }
}

#[cfg(test)]
mod tests
{
    use super::*;
    use std::time::Instant;
    use wiremock::matchers::{bearer_token, body_string, body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_authenticate() -> Result<()>
    {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/mock-tenant-id/oauth2/v2.0/token"))
            .and(header("content-type", "application/x-www-form-urlencoded"))
            .and(body_string_contains("client_id=mock-client-id"))
            .and(body_string_contains("client_secret=mock-client-secret"))
            .and(body_string_contains("scope=https%3A%2F%2Fgraph.microsoft.com%2F.default"))
            .and(body_string_contains("grant_type=client_credentials"))
            .respond_with(ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "token_type": "Bearer", "access_token": "mock-access-token", "expires_in": 3600 })))
            .mount(&server)
            .await;

        let config = Config::new(
            "mock-tenant-id",
            "mock-client-id",
            "mock-client-secret",
        )
            .with_login_endpoint(server.uri())
            .with_graph_endpoint(server.uri());

        let mut client = Client::new(config);
        client.authenticate().await?;

        // assert we're authenticated and token is as we expect
        let token = client.get_authentication();
        if let Ok(token) = token {
            assert_eq!(token.access_token(), "mock-access-token");
            assert!(!token.is_expired());
        } else {
            panic!("client not authenticated as expected");
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_send_mail() -> Result<()>
    {
        let mime_data = b"From: alice@example.com\r\nTo: bob@example.com\r\nSubject: Test\r\n\r\nHello Bob.\r\n";
        let mime_base64 = BASE64.encode(mime_data);

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/users/alice@example.com/sendMail"))
            .and(header("content-type", "text/plain"))
            .and(bearer_token("mock-access-token"))
            .and(body_string(mime_base64))
            .respond_with(ResponseTemplate::new(200).set_body_string("202 Accepted"))
            .mount(&server)
            .await;

        let config = Config::new(
            "mock-tenant-id",
            "mock-client-id",
            "mock-client-secret",
        )
            .with_login_endpoint(server.uri())
            .with_graph_endpoint(server.uri());

        let mut client = Client {
            config,
            token: Some(
                AccessToken::new_for_test(
                    "Bearer",
                    "mock-access-token",
                    3600,
                    Instant::now())
            ),
        };
        assert!(client.get_authentication().is_ok());

        client.send_mail("alice@example.com", mime_data).await?;

        Ok(())
    }
}
