//! # Graphcast
//!
//! `graphcast-sdk` is a development kit to build The Graph network
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

use ethers::signers::{Signer, Wallet};
use ethers_core::k256::ecdsa::SigningKey;
use once_cell::sync::Lazy;
use once_cell::sync::OnceCell;
use std::fmt;
use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
};
use tracing::{debug, subscriber::SetGlobalDefaultError, Level};
use tracing_subscriber::FmtSubscriber;
use url::{Host, Url};

pub mod graphcast_agent;
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
pub fn discovery_url() -> Result<Url, url::ParseError> {
    let enr_url = config_env_var("ENR_URL").unwrap_or_else(|_| {
        "enrtree://AMRFINDNF7XHQN2XBYCGYAYSQ3NV77RJIHLX6HJLA6ZAF365NRLMM@testfleet.graphcast.xyz"
            .to_string()
    });

    Url::parse(&enr_url)
}

pub fn cf_nameserver() -> Host {
    Host::Domain("konnor.ns.cloudflare.com".to_string())
}

/// Attempt to read environmental variable
pub fn config_env_var(name: &str) -> Result<String, String> {
    env::var(name).map_err(|e| format!("{name}: {e}"))
}

/// Get the graphcastID addresss associated with a given Indexer address
pub fn graphcast_id_address(wallet: &Wallet<SigningKey>) -> String {
    debug!("{}", format!("Wallet address: {:?}", wallet.address()));
    format!("{:?}", wallet.address())
}

/// Helper function to parse boot node addresses from the environment variables
/// Defaults to an empty vec if it cannot find the 'BOOT_NODE_ADDRESSES' environment variable
/// Multiple formats for defining the addresses list are supported, such as:
/// 1. BOOT_NODE_ADDRESSES=[addr1, addr2, addr3]
/// 2. BOOT_NODE_ADDRESSES="addr1", "addr2", "addr3"
/// 3. BOOT_NODE_ADDRESSES="addr1, addr2, addr3"
/// 4. BOOT_NODE_ADDRESSES=addr1, addr2, addr3
/// 5. BOOT_NODE_ADDRESSES=addr
/// 6. BOOT_NODE_ADDRESSES="[addr1, addr2, addr3]"
pub fn read_boot_node_addresses() -> Vec<String> {
    let mut addresses = Vec::new();
    if let Ok(val) = env::var("BOOT_NODE_ADDRESSES") {
        for address in val.split(',') {
            let address = address.trim();
            if address.starts_with('"') && address.ends_with('"') {
                addresses.push(address[1..address.len() - 1].to_string());
            } else {
                addresses.push(address.to_string());
            }
        }
    }
    addresses
}

/// Struct for a block pointer
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BlockPointer {
    pub number: u64,
    pub hash: String,
}

impl BlockPointer {
    pub fn new(number: u64, hash: String) -> Self {
        BlockPointer { number, hash }
    }
}

/// Struct for Network and block interval for updates
#[derive(Debug, Clone)]
pub struct Network {
    pub name: NetworkName,
    pub interval: u64,
}

/// List of supported networks
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum NetworkName {
    Goerli,
    Mainnet,
    Gnosis,
    Hardhat,
    ArbitrumOne,
    ArbitrumGoerli,
    Avalanche,
    Polygon,
    Celo,
    Optimism,
    Unknown,
}

impl NetworkName {
    pub fn from_string(name: &str) -> Self {
        match name {
            "goerli" => NetworkName::Goerli,
            "mainnet" => NetworkName::Mainnet,
            "gnosis" => NetworkName::Gnosis,
            "hardhat" => NetworkName::Hardhat,
            "arbitrum-one" => NetworkName::ArbitrumOne,
            "arbitrum-goerli" => NetworkName::ArbitrumGoerli,
            "avalanche" => NetworkName::Avalanche,
            "polygon" => NetworkName::Polygon,
            "celo" => NetworkName::Celo,
            "optimism" => NetworkName::Optimism,
            _ => NetworkName::Unknown,
        }
    }
}

impl fmt::Display for NetworkName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            NetworkName::Goerli => "goerli",
            NetworkName::Mainnet => "mainnet",
            NetworkName::Gnosis => "gnosis",
            NetworkName::Hardhat => "hardhat",
            NetworkName::ArbitrumOne => "arbitrum-one",
            NetworkName::ArbitrumGoerli => "arbitrum-goerli",
            NetworkName::Avalanche => "avalanche",
            NetworkName::Polygon => "polygon",
            NetworkName::Celo => "celo",
            NetworkName::Optimism => "optimism",
            NetworkName::Unknown => "unknown",
        };

        write!(f, "{name}")
    }
}

/// Maintained static list of supported Networks
pub static NETWORKS: Lazy<Vec<Network>> = Lazy::new(|| {
    vec![
        Network {
            name: NetworkName::from_string("goerli"),
            interval: 2,
        },
        Network {
            name: NetworkName::from_string("mainnet"),
            interval: 10,
        },
        Network {
            name: NetworkName::from_string("gnosis"),
            interval: 5,
        },
        Network {
            name: NetworkName::from_string("hardhat"),
            interval: 5,
        },
        Network {
            name: NetworkName::from_string("arbitrum-one"),
            interval: 5,
        },
        Network {
            name: NetworkName::from_string("arbitrum-goerli"),
            interval: 5,
        },
        Network {
            name: NetworkName::from_string("avalanche"),
            interval: 5,
        },
        Network {
            name: NetworkName::from_string("polygon"),
            interval: 5,
        },
        Network {
            name: NetworkName::from_string("celo"),
            interval: 5,
        },
        Network {
            name: NetworkName::from_string("optimism"),
            interval: 5,
        },
    ]
});

/// Sets up tracing, allows log level to be set from the environment variables
pub fn init_tracing() -> Result<(), SetGlobalDefaultError> {
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
    tracing::subscriber::set_global_default(subscriber)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphcast_agent::waku_handling::build_content_topics;

    #[test]
    fn test_build_content_topics() {
        let basics = ["Qmyumyum".to_string(), "Ymqumqum".to_string()].to_vec();
        let res = build_content_topics("some-radio", 0, &basics);
        for i in 0..res.len() {
            assert_eq!(res[i].content_topic_name, basics[i]);
            assert_eq!(res[i].application_name, "some-radio");
        }
    }

    #[test]
    fn test_read_boot_node_addresses_empty() {
        std::env::remove_var("BOOT_NODE_ADDRESSES");
        let items = read_boot_node_addresses();
        assert_eq!(items, Vec::<String>::new());
    }

    #[test]
    fn test_read_boot_node_addresses_no_quotes() {
        std::env::set_var("BOOT_NODE_ADDRESSES", "addr1, addr2, addr3");
        let items = read_boot_node_addresses();
        assert_eq!(items, vec!["addr1", "addr2", "addr3"]);
    }

    #[test]
    fn test_read_boot_node_addresses_single_item_no_quotes() {
        std::env::set_var("BOOT_NODE_ADDRESSES", "addr1");
        let items = read_boot_node_addresses();
        assert_eq!(items, vec!["addr1"]);
    }

    #[test]
    fn test_read_boot_node_addresses_single_item_with_quotes() {
        std::env::set_var("BOOT_NODE_ADDRESSES", r#""addr1""#);
        let items = read_boot_node_addresses();
        assert_eq!(items, vec!["addr1"]);
    }

    #[test]
    fn test_read_boot_node_addresses_with_quotes() {
        std::env::set_var("BOOT_NODE_ADDRESSES", r#""addr1", "addr2", "addr3""#);
        let items = read_boot_node_addresses();
        assert_eq!(items, vec!["addr1", "addr2", "addr3"]);
    }

    #[test]
    fn test_read_boot_node_addresses_with_commas_no_quotes() {
        std::env::set_var("BOOT_NODE_ADDRESSES", "addr1,addr2,addr3");
        let items = read_boot_node_addresses();
        assert_eq!(items, vec!["addr1", "addr2", "addr3"]);
    }

    #[test]
    fn test_read_boot_node_addresses_with_commas_and_quotes() {
        std::env::set_var("BOOT_NODE_ADDRESSES", r#""addr1","addr2","addr3""#);
        let items = read_boot_node_addresses();
        assert_eq!(items, vec!["addr1", "addr2", "addr3"]);
    }
}
