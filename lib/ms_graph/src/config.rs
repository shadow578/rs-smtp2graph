use crate::GRAPH_API_TIMEOUT;

/// Default api endpoint for OAuth2 login api.
pub const DEFAULT_LOGIN_ENDPOINT: &str = "https://login.microsoftonline.com";

/// Default api endpoint for microsoft graph api.
pub const DEFAULT_GRAPH_ENDPOINT: &str = "https://graph.microsoft.com/v1.0";

/// M365 Graph client configuration.
#[derive(Debug, Clone)]
pub struct Config
{
    /// id of the m365 tenant id the application is registered in.
    tenant_id: String,

    /// id of the m365 app registration.
    client_id: String,

    /// client secret for the app registration.
    client_secret: String,

    /// login api base url.
    login_endpoint: String,

    /// graph api base url.
    graph_endpoint: String,

    /// HTTP client instance.
    http_client: reqwest::Client,
}

impl Config
{
    /// construct a new graph client configuration.
    /// tenant_id: id of the m365 tenant id the application is registered in.
    /// client_id: id of the m365 app registration.
    /// client_secret: client secret for the app registration.
    pub fn new<T>(tenant_id: T, client_id: T, client_secret: T) -> Self
    where
        T: Into<String>,
    {
        let http_client = reqwest::Client::builder()
            .timeout(GRAPH_API_TIMEOUT)
            .build()
            .unwrap();

        Config {
            tenant_id: tenant_id.into(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            login_endpoint: DEFAULT_LOGIN_ENDPOINT.into(),
            graph_endpoint: DEFAULT_GRAPH_ENDPOINT.into(),
            http_client,
        }
    }

    /// override login api base url.
    /// default is DEFAULT_LOGIN_ENDPOINT.
    /// login_endpoint: base url for ms login api endpoint.
    pub fn with_login_endpoint<T>(mut self, login_endpoint: T) -> Self
    where
        T: Into<String>,
    {
        self.login_endpoint = login_endpoint.into();
        self
    }

    /// override graph api base url.
    /// default is DEFAULT_GRAPH_ENDPOINT.
    /// graph_endpoint: base url for ms graph api endpoint.
    pub fn with_graph_endpoint<T>(mut self, graph_endpoint: T) -> Self
    where
        T: Into<String>,
    {
        self.graph_endpoint = graph_endpoint.into();
        self
    }

    /// set a custom http client to use.
    /// http_client: http client instance to use.
    pub fn with_http_client(mut self, http_client: reqwest::Client) -> Self {
        self.http_client = http_client;
        self
    }

    /// get the tenant id.
    pub(crate) fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    /// get the client id.
    pub(crate) fn client_id(&self) -> &str {
        &self.client_id
    }

    /// get the client secret.
    pub(crate) fn client_secret(&self) -> &str {
        &self.client_secret
    }

    /// get the base url for login api.
    pub(crate) fn login_endpoint(&self) -> &str {
        &self.login_endpoint
    }

    /// get the base url for ms graph api.
    pub(crate) fn graph_endpoint(&self) -> &str {
        &self.graph_endpoint
    }

    /// get http client.
    pub(crate) fn http_client(&self) -> &reqwest::Client {
        &self.http_client
    }
}