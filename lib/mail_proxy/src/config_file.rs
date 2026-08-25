use crate::custom_serde::opt_duration_secs;
use crate::defaults::{DEFAULT_FAIL2BAN_CONNECTIONS, DEFAULT_FAIL2BAN_DURATION, DEFAULT_FAIL2BAN_FAILS, DEFAULT_SMTP_BIND_ADDRESS};
use anyhow::{Result, anyhow};
use argon2::{
    Argon2,
    password_hash::{
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
        rand_core::OsRng,
    },
};
use log::debug;
use ms_graph::RECOMMENDED_MAX_MESSAGE_SIZE;
use ms_graph::config::Config as MSGraphCrateConfig;
use serde::{Deserialize, Serialize};
use smtp::AuthMode;
use smtp::config::Config as SMTPCrateConfig;
use smtp::handler::Handler;
use smtp::tls_config::TlsConfig;
use std::collections::HashMap;
use std::collections::hash_map::Keys;
use std::time::Duration;
use tokio::fs;

/// mail proxy configuration file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFile
{
    /// SMTP server config.
    pub smtp: SMTPServerConfig,

    /// MS Graph client config.
    pub graph: GraphAPIConfig,
}

impl ConfigFile
{
    /// create an empty configuration file
    pub fn empty() -> Self
    {
        Self {
            smtp: SMTPServerConfig {
                address: DEFAULT_SMTP_BIND_ADDRESS.into(),
                name: None,
                max_message_size: None,
                allow_insecure_auth: false,
                tls: None,
                fail2ban: None,
                users: HashMap::new(),
            },
            graph: GraphAPIConfig {
                tenant_id: String::new(),
                client_id: String::new(),
                client_secret: String::new(),
            },
        }
    }

    /// read configuration data from .yaml file.
    /// path: file path to read config from.
    pub async fn from_file(path: &str) -> Result<Self>
    {
        debug!("Loading configuration from {}", path);
        let yaml = fs::read_to_string(path).await?;
        yaml_serde::from_str(&yaml)
            .map_err(|e| anyhow!("{}", e))
    }

    /// write configuration data to .yaml file.
    /// path: file path to write to.
    pub async fn to_file(&self, path: &str) -> Result<()>
    {
        debug!("Writing configuration to {}", path);
        let yaml = yaml_serde::to_string(self)?;
        fs::write(path, yaml).await?;
        Ok(())
    }
}

/// defines configuration options for the SMTP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SMTPServerConfig
{
    /// listen address, e.g. 0.0.0.0:25.
    pub address: String,

    /// name of the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// maximum message size, in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_message_size: Option<usize>,

    /// allow user authentication even when TLS is not configured.
    pub allow_insecure_auth: bool,

    /// configuration for StartTLS.
    /// if Some, auth will require TLS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls: Option<TLSConfig>,

    /// configuration for fail2ban mechanism.
    /// if None, fail2ban will use defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail2ban: Option<Fail2BanConfig>,

    /// user authentication and mapping details.
    /// map key is username, entry contains password and metadata.
    /// username must equal a M365 user upn that is in-scope for the graph app.
    /// if no users are configured, authentication will be disabled.
    users: HashMap<String /* username */, String /* hash */>,
}

impl SMTPServerConfig
{
    /// apply the configuration options defined in this config to the existing SMTP server config object.
    /// config: server config object from smtp crate.
    pub(crate) fn apply_to_server_config<H>(&self, config: &mut SMTPCrateConfig<H>) -> Result<()>
    where
        H: Handler,
    {
        config.with_address(self.address.clone());
        config.with_server_name(self.name.as_ref().unwrap_or(&"smtp2graph".into()));

        if let Some(tls) = &self.tls {
            config.with_tls(TlsConfig::Chain {
                certificates: tls.certificate_chain.clone(),
                private_key: tls.private_key.clone(),
            })?;
        }

        config.with_auth(
            if self.has_users() {
                if self.tls.is_some() { AuthMode::RequireTls } else {
                    if self.allow_insecure_auth { AuthMode::Always } else { AuthMode::None }
                }
            } else { AuthMode::None }
        );

        config.with_max_message_size(self.max_message_size.unwrap_or(RECOMMENDED_MAX_MESSAGE_SIZE));

        Ok(())
    }


    /// get effective configuration for fail2ban.
    pub fn get_effective_fail2ban_config(&self) -> (u32 /* max_connections */, u32 /* max_failures */, Duration /* ban_duration */)
    {
        let config = self.fail2ban.as_ref().unwrap_or(&Fail2BanConfig {
            max_connections: None,
            max_failures: None,
            ban_duration: None,
        });

        (
            config.max_connections.unwrap_or(DEFAULT_FAIL2BAN_CONNECTIONS),
            config.max_failures.unwrap_or(DEFAULT_FAIL2BAN_FAILS),
            config.ban_duration.unwrap_or(DEFAULT_FAIL2BAN_DURATION),
        )
    }

    /// add or update user entry.
    /// username: username to add or modify.
    /// password: new password to set.
    pub fn set_user_password(&mut self, username: String, password: String) -> Result<()>
    {
        debug!("Updating user password for {}", username);

        let salt = SaltString::generate(&mut OsRng);
        let password = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| anyhow!("could not set password"))?
            .to_string();

        self.users.insert(username, password);
        Ok(())
    }

    /// remove an existing user entry.
    /// username: username to remove.
    pub fn remove_user(&mut self, username: String) -> Result<()>
    {
        debug!("Removing user {}", username);

        self.users.remove(&username).ok_or_else(|| anyhow!("User not found"))?;
        Ok(())
    }

    /// check if a user exists.
    /// username: the username to check for.
    pub fn has_user(&self, username: String) -> bool
    {
        self.users.contains_key(&username)
    }

    /// are any users configured, enabling authentication?
    pub fn has_users(&self) -> bool
    {
        !self.users.is_empty()
    }

    /// get a list of all users.
    pub fn list_users(&self) -> Keys<'_, String, String>
    {
        self.users.keys()
    }

    /// verify username exists and password is correct.
    /// username: username to match to.
    /// password: clear-text password to validate is correct.
    pub(crate) fn verify_user_password(&self, username: String, password: String) -> Result<()>
    {
        let hash = self.users.get(&username).ok_or_else(|| anyhow!("User {} not found", username))?;
        let hash = PasswordHash::new(hash.as_str())
            .map_err(|_| anyhow!("could not parse password hash string"))?;

        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .map_err(|_| anyhow!("invalid password"))?;

        Ok(())
    }
}

/// defines configuration options for StartTLS support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TLSConfig
{
    /// paths of certificates of the cert chain to use.
    /// for self-signed, provide only the single certificate.
    pub certificate_chain: Vec<String>,

    /// private key file path.
    pub private_key: String,
}


/// define configuration options for SMTP server fail2ban mechanism.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fail2BanConfig
{
    /// maximum number of active connections per peer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_connections: Option<u32>,

    /// maximum number of failures per peer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_failures: Option<u32>,

    /// how long peers are banned for violating limits.
    #[serde(skip_serializing_if = "Option::is_none", with = "opt_duration_secs", default)]
    pub ban_duration: Option<Duration>,
}

/// defines configuration for Graph API client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphAPIConfig
{
    /// ID of the M365 tenant the application is registered in.
    pub tenant_id: String,

    /// ID of the M365 app / client registration.
    pub client_id: String,

    /// client secret used for authentication against M365 Graph API.
    pub client_secret: String,
}

impl GraphAPIConfig
{
    /// get ms_graph crate config for this configuration.
    pub fn into_client_config(self) -> MSGraphCrateConfig
    {
        MSGraphCrateConfig::new(self.tenant_id, self.client_id, self.client_secret)
    }
}
