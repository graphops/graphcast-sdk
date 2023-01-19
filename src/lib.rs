//! # Graphcast
//!
//! `graphcast` is a development kit to build The Graph network
//! gossip p2p messaging apps on top of the Waku Relay network.
//!
//! This library is a work in progress, in particular
//! operating on Goerli testnet and Waku Rust Bindings.
//!
//! ## Getting Started
//!
//! Add this crate to your project's `Cargo.toml`.
//!
//! ## Examples and Usage
//!
//! Check out the examples folder for helpful snippets of code, as well as minimal configurations.
//! For more explanation, see the crate documentation.
//!

use once_cell::sync::OnceCell;
use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;
use url::{Host, Url};

pub mod gossip_agent;
pub mod graphql;
pub mod slack_bot;

type NoncesMap = HashMap<String, HashMap<String, i64>>;

/// Each radio persist Nonces from sub-topic messages differentiating sender
pub static NONCES: OnceCell<Arc<Mutex<NoncesMap>>> = OnceCell::new();

/// Returns Graphcast application domain name
///
/// Example
///
/// ```
/// let mut f = graphcast_sdk::app_name();
/// assert_eq!(f, "graphcast".to_string());
/// ```
pub fn app_name() -> Cow<'static, str> {
    Cow::from("graphcast")
}

/// Returns hardcoded DNS Url to a discoverable ENR tree that should be used to retrieve boot nodes
pub fn discovery_url() -> Url {
    let enr_url = config_env_var("ENR_URL").unwrap_or_else(|_| {
        "enrtree://AMRFINDNF7XHQN2XBYCGYAYSQ3NV77RJIHLX6HJLA6ZAF365NRLMM@testfleet.graphcast.xyz"
            .to_string()
    });

    Url::parse(&enr_url).expect("Could not parse discovery url to ENR tree")
}

pub fn cf_nameserver() -> Host {
    Host::Domain("konnor.ns.cloudflare.com".to_string())
}

/// Attempt to read environmental variable
pub fn config_env_var(name: &str) -> Result<String, String> {
    env::var(name).map_err(|e| format!("{}: {}", name, e))
}

/// Sets up tracing, allows log level to be set from the environment variables
pub fn init_tracing() {
    let log_level = match env::var("LOG_LEVEL") {
        Ok(level) => match level.to_uppercase().as_str() {
            "INFO" => Level::INFO,
            "DEBUG" => Level::DEBUG,
            "TRACE" => Level::TRACE,
            "WARN" => Level::WARN,
            "ERROR" => Level::ERROR,
            _ => Level::INFO,
        },
        Err(_) => Level::INFO,
    };

    let subscriber = FmtSubscriber::builder().with_max_level(log_level).finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();
}

#[cfg(test)]
mod tests {
    use crate::gossip_agent::waku_handling::build_content_topics;

    #[test]
    fn test_build_content_topics() {
        let basics = ["Qmyumyum", "Ymqumqum"].to_vec();
        let res = build_content_topics("some-radio", 0, &basics);
        for i in 0..res.len() {
            assert_eq!(res[i].content_topic_name, basics[i]);
            assert_eq!(res[i].application_name, "some-radio");
        }
    }
}
