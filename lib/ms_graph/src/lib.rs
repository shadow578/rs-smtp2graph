use std::time::Duration;

pub mod config;
pub mod client;
mod auth;


/// maximum email message size that is known to work reliably.
/// while Microsoft's API docs don't actually define a limit, through testing
/// I found that the graph API will fail to accept mails at about 50MB.
pub const API_MAX_MESSAGE_SIZE: usize = 50 * 1024 * 1024;

/// recommended maximum email size to ensure delivery.
/// in addition to MAX_MESSAGE_SIZE, this takes into consideration that,
/// by default, Exchange online limits sent messages to around 35 MB.
pub const RECOMMENDED_MAX_MESSAGE_SIZE: usize = 34 * 1024 * 1024;

const _: () = assert!(RECOMMENDED_MAX_MESSAGE_SIZE <= API_MAX_MESSAGE_SIZE);

/// default connection timeout to graph api
pub const GRAPH_API_TIMEOUT: Duration = Duration::from_secs(60);