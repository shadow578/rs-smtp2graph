use crate::util::{existing_file, mask_string, prompt_user_confirmation};
use base64::Engine;
use clap::{ArgGroup, Parser};
use humantime::{format_duration, parse_duration};
use mail_proxy::config_file::{ConfigFile, Fail2BanConfig, TLSConfig};
use ms_graph::client::Client as GraphClient;
use ms_graph::{API_MAX_MESSAGE_SIZE, RECOMMENDED_MAX_MESSAGE_SIZE};
use std::time::Duration;

/// SMTP to Microsoft Graph API mail proxy by Chris.
#[derive(Parser, Debug)]
pub(crate) struct Cli
{
    /// Path to the configuration file.
    #[arg(short, long, default_value = "config.yaml")]
    pub(crate) config: String,

    #[command(subcommand)]
    pub(crate) command: Option<CliCommand>,
}

#[derive(Parser, Debug)]
pub(crate) enum CliCommand
{
    /// Run smtp2graph.
    Run
    {
        /// Run smtp2graph as a Windows service.
        /// Please note that in this mode, you must provide the absolute config path via '--config'.
        #[cfg(windows)]
        #[clap(short, long)]
        service: bool
    },

    /// Update the configuration.
    Config {
        #[clap(subcommand)]
        command: ConfigCommand
    },
}

#[derive(Parser, Debug)]
pub(crate) enum ConfigCommand
{
    /// SMTP Server configuration.
    Smtp {
        #[command(subcommand)]
        command: SmtpCommand
    },

    /// Microsoft Graph API configuration.
    Graph {
        #[command(subcommand)]
        command: GraphCommand
    },

    /// User authentication configuration.
    User {
        #[command(subcommand)]
        command: UserCommand
    },

    /// Restore default configuration.
    Reset,
}

impl ConfigCommand
{
    pub(crate) async fn execute(&self, config: &mut ConfigFile)
    {
        match self {
            ConfigCommand::Smtp { command } => command.execute(config).await,
            ConfigCommand::Graph { command } => command.execute(config).await,
            ConfigCommand::User { command } => command.execute(config).await,
            ConfigCommand::Reset => {
                let defaults = ConfigFile::empty();
                config.smtp = defaults.smtp;
                config.graph = defaults.graph;
            }
        }
    }
}

#[derive(Parser, Debug)]
pub(crate) enum SmtpCommand
{
    /// Show current SMTP server configuration.
    #[command()]
    Show,

    /// Update SMTP server configuration.
    #[command(group(
        ArgGroup::new("update")
            .required(true)
            .multiple(true)
            .args(["address", "name", "max_message_size"])
    ))]
    Update {
        /// SMTP server listen address. Example: '0.0.0.0:25'.
        #[arg(short, long)]
        address: Option<String>,

        /// SMTP server name.
        #[arg(short, long)]
        name: Option<String>,

        /// Maximum E-Mail message size, in bytes.
        #[arg(short, long)]
        max_message_size: Option<usize>,
    },

    /// Update SMTP server fail2ban configuration.
    #[command(name = "fail2ban",
        group(
        ArgGroup::new("update")
            .required(true)
            .multiple(true)
            .args(["connections", "failures", "duration", "reset"])
        ))]
    Fail2Ban {
        /// Maximum number of connections a client is allowed to hold.
        #[arg(short, long)]
        connections: Option<u32>,

        /// Maximum number of suspicious / failed sessions before a client's IP is banned.
        #[arg(short, long)]
        failures: Option<u32>,

        /// For how long a client's IP remains banned.
        #[arg(short, long, value_parser = parse_duration)]
        duration: Option<Duration>,

        /// Reset to default settings.
        #[arg(long)]
        reset: bool,
    },

    /// Allow authentication over unsecure connections.
    #[command(group(
        ArgGroup::new("allow")
            .required(true)
            .multiple(false)
            .args(["yes", "no"])
    ))]
    AllowInsecureAuth {
        /// Allow authentication over plain text.
        #[arg(short, long)]
        yes: bool,

        /// Allow authentication only over TLS connection.
        #[arg(short, long)]
        no: bool,
    },

    /// Configure SMTP server TLS configuration
    #[command()]
    Tls {
        #[command(subcommand)]
        command: TlsCommand
    },
}

impl SmtpCommand
{
    pub(crate) async fn execute(&self, config: &mut ConfigFile)
    {
        match self {
            SmtpCommand::Show => {
                Self::show(config);
            }
            SmtpCommand::Update { address, name, max_message_size } => {
                if let Some(address) = address {
                    config.smtp.address = address.into();
                }

                if let Some(name) = name {
                    config.smtp.name = Some(name.into());
                }

                if let Some(max_message_size) = max_message_size {
                    config.smtp.max_message_size = if *max_message_size == 0 {
                        None
                    } else {
                        Some(*max_message_size)
                    }
                }

                // disable insecure auth when not needed
                if config.smtp.allow_insecure_auth && (config.smtp.tls.is_some() || !config.smtp.auth.has_users()) {
                    config.smtp.allow_insecure_auth = false;
                }

                Self::show(config);
            }
            SmtpCommand::Fail2Ban { connections, failures, duration, reset } => {
                config.smtp.fail2ban = if *reset {
                    None
                } else {
                    let mut fail2ban =
                        if let Some(fail2ban) = config.smtp.fail2ban.clone() { fail2ban } else { Fail2BanConfig { max_connections: None, max_failures: None, ban_duration: None } };

                    if let Some(connections) = connections {
                        fail2ban.max_connections = Some(*connections);
                    }

                    if let Some(failures) = failures {
                        fail2ban.max_failures = Some(*failures);
                    }

                    if let Some(duration) = duration {
                        fail2ban.ban_duration = Some(*duration)
                    }

                    Some(fail2ban)
                };

                Self::show(config);
            }
            SmtpCommand::AllowInsecureAuth { yes, no } => {
                if *no {
                    config.smtp.allow_insecure_auth = false;
                } else if *yes {
                    println!("WARNING: You're about to allow authentication over unsecure, plain-text connections.");
                    println!("In this configuration, credentials are sent in plain-text, potentially allowing credential theft.");

                    if prompt_user_confirmation("yes, i understand").is_ok() {
                        println!("Insecure auth enabled");
                        config.smtp.allow_insecure_auth = true;
                    } else {
                        println!("Aborting");
                    }
                }
            }
            SmtpCommand::Tls { command } => {
                command.execute(config).await;
                Self::show(config);
            }
        }
    }

    fn show(config: &ConfigFile)
    {
        println!("SMTP Server Configuration:");
        println!(" Listen Address: {}", config.smtp.address);
        println!(" Server Name: {}", config.smtp.name.clone().unwrap_or("N/A".into()));

        print!(" Maximum Message Size: ");
        if let Some(max_message_size) = config.smtp.max_message_size {
            print!("{} bytes", max_message_size);

            if max_message_size > API_MAX_MESSAGE_SIZE {
                println!(" (above maximum of {API_MAX_MESSAGE_SIZE} bytes the Graph API can reliably handle)")
            } else if max_message_size > RECOMMENDED_MAX_MESSAGE_SIZE {
                println!(" (above recommended maximum of {RECOMMENDED_MAX_MESSAGE_SIZE} bytes)")
            } else {
                println!();
            }
        } else {
            println!("Automatic");
        }

        let (max_connections, max_failures, ban_duration) = config.smtp.get_effective_fail2ban_config();
        println!();
        println!("Fail2Ban Configuration:");
        println!(" Maximum Connections: {}", max_connections);
        println!(" Max Failures: {}", max_failures);
        println!(" Ban Duration: {}", format_duration(ban_duration));

        println!();
        print!("TLS Configuration:");
        if let Some(tls) = &config.smtp.tls
        {
            println!();
            println!(" Certificate Chain: ");
            for cert in &tls.certificate_chain {
                println!("   {}", cert);
            }

            println!(" Private Key: {}", tls.private_key);
        } else {
            println!(" N/A");
        }

        if config.smtp.tls.is_none() && config.smtp.auth.has_users() {
            println!();
            println!("WARNING: You've configured user authentication, but have not configured TLS.");

            if config.smtp.allow_insecure_auth {
                println!("In this configuration, credentials are sent in plain-text, potentially allowing credential theft.");
            } else {
                println!("Authentication is currently not enabled.");
                println!("To enable authentication over plain-text connections, run 'smtp2graph config smtp allow-insecure-auth --yes'")
            }
        }
    }
}

#[derive(Parser, Debug)]
pub(crate) enum TlsCommand
{
    /// Configure TLS.
    #[command()]
    Setup {
        /// Path to certificate(s) for certificate chain, most concrete listed first.
        /// For a self-signed certificate, you'll only need one.
        #[arg(short, long, value_parser = existing_file)]
        certificate: Vec<String>,

        /// Private key for TLS certificate.
        #[arg(short, long, value_parser = existing_file)]
        private_key: String,
    },

    /// Disable TLS.
    #[command()]
    Disable,
}

impl TlsCommand
{
    pub(crate) async fn execute(&self, config: &mut ConfigFile)
    {
        match self {
            TlsCommand::Setup { certificate, private_key } => {
                config.smtp.tls = Some(TLSConfig {
                    certificate_chain: certificate.clone(),
                    private_key: private_key.clone(),
                })
            }
            TlsCommand::Disable => {
                config.smtp.tls = None;
            }
        }
    }
}

#[derive(Parser, Debug)]
pub(crate) enum GraphCommand
{
    /// Show current Microsoft Graph API configuration.
    #[command()]
    Show,

    /// Update Microsoft Graph API configuration.
    #[command(group(
        ArgGroup::new("update")
            .required(true)
            .multiple(true)
            .args(["tenant_id", "client_id", "client_secret"])
    ))]
    Update {
        /// ID of the Microsoft Entra tenant the application is registered in.
        #[arg(long)]
        tenant_id: Option<String>,

        /// ID of the Microsoft Entra app / client registration.
        #[arg(long)]
        client_id: Option<String>,

        /// Client secret used for authentication against Microsoft Graph API.
        #[arg(long)]
        client_secret: Option<String>,
    },

    /// Test connection to Microsoft Graph API.
    #[command()]
    Test,
}

impl GraphCommand
{
    pub(crate) async fn execute(&self, config: &mut ConfigFile)
    {
        match self {
            GraphCommand::Show => {
                Self::show(config);
            }
            GraphCommand::Update { tenant_id, client_id, client_secret } => {
                if let Some(tenant_id) = tenant_id {
                    config.graph.tenant_id = tenant_id.into();
                }

                if let Some(client_id) = client_id {
                    config.graph.client_id = client_id.into();
                }

                if let Some(client_secret) = client_secret {
                    config.graph.client_secret = client_secret.into();
                }

                Self::show(config);
                println!();
                Self::test(config).await;
            }
            GraphCommand::Test => {
                Self::test(config).await;
            }
        }
    }

    fn show(config: &ConfigFile)
    {
        println!("Microsoft Graph API Configuration:");
        println!(" Tenant ID: {}", mask_string(&config.graph.tenant_id, 6));
        println!(" Client ID: {}", mask_string(&config.graph.client_id, 6));
        println!(" Client Secret: {}", mask_string(&config.graph.client_secret, 6));
    }

    async fn test(config: &ConfigFile)
    {
        println!("Testing connection to Microsoft Graph API...");

        let graph_config = config.graph.clone().into_client_config();
        let mut client = GraphClient::new(graph_config);

        match client.authenticate().await
        {
            Ok(_) => {
                println!("Connected to Microsoft Graph successfully");
            }
            Err(err) => {
                eprintln!("Error connecting to Microsoft Graph: {}", err);
            }
        }
    }
}

#[derive(Parser, Debug)]
pub(crate) enum UserCommand
{
    /// Show all currently configured users.
    #[command()]
    Show,

    /// Add a new user.
    #[command()]
    Add {
        /// Username to add. Must match Microsoft Entra User UPN.
        username: String,

        /// Password for authentication against mail proxy.
        password: Option<String>,

        /// force accept the username, even if it is likely invalid.
        #[arg(long)]
        force: bool,
    },

    /// Remove an existing user.
    #[command()]
    Remove {
        /// Username to remove.
        username: String,
    },
}

impl UserCommand
{
    pub(crate) async fn execute(&self, config: &mut ConfigFile)
    {
        match self {
            UserCommand::Show => {
                Self::show(config);
            }
            UserCommand::Add { username, password, force } => {
                println!("{} user {}",
                         if config.smtp.auth.has_user(username) { "Updating" } else { "Adding" },
                         username
                );

                if !username.contains("@")
                {
                    println!("Username does not look like a Microsoft 365 username.");
                    println!("Please note that the username *must* match the username in M365.");

                    if !force
                    {
                        return;
                    }
                }

                let password = match password
                {
                    Some(password) => password.into(),
                    None => {
                        // auto-generate a password
                        // 24 bytes / 32 chars = 192 bits entropy
                        let mut pwd = [0u8; 24];
                        rand::fill(&mut pwd);
                        let pwd = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(pwd);

                        println!("Password: {}", pwd);
                        pwd
                    }
                };

                if let Err(err) = config.smtp.auth.set_user_password(username, &password)
                {
                    eprintln!("Failed to update user: {}", err);
                }

                println!();
                Self::show(config);
            }
            UserCommand::Remove { username } => {
                println!("Removing user {}", username);
                if let Err(err) = config.smtp.auth.remove_user(username)
                {
                    eprintln!("Failed to remove user: {}", err);
                }

                println!();
                Self::show(config);
            }
        }
    }

    fn show(config: &ConfigFile)
    {
        let users = config.smtp.auth.list_users();

        println!("Listing {} Users:", users.len());
        for user in users
        {
            println!(" {}", user);
        }
    }
}

