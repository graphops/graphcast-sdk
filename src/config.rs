use clap::Parser;
use ethers::signers::{LocalWallet, WalletError};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::read_to_string;
use std::str::FromStr;
use tracing::{debug, info};
use waku::Multiaddr;

use crate::graphql::client_graph_node::get_indexing_statuses;
use crate::graphql::client_network::query_network_subgraph;
use crate::graphql::client_registry::query_registry_indexer;
use crate::{graphcast_id_address, init_tracing};

#[derive(Clone, Debug, Parser, Serialize, Deserialize)]
#[clap(
    name = "graphcast",
    about = "Gossip realtime information for the emerging data marketplace",
    author = "GraphOps"
)]
pub struct Config {
    #[clap(
        long,
        env = "GRAPHCAST_CONFIG",
        help = "path to the Graphcast radio configuration file (Unimplemented)"
    )]
    pub config_file: Option<String>,
    #[clap(
        long,
        value_name = "ENDPOINT",
        env = "GRAPH_NODE_STATUS_ENDPOINT",
        help = "API endpoint to the Graph Node Status Endpoint"
    )]
    pub graph_node_endpoint: String,
    #[clap(
        long,
        value_name = "KEY",
        value_parser = Config::parse_key,
        env = "PRIVATE_KEY",
        hide_env_values = true,
        help = "Private key to the Graphcase ID wallet",
    )]
    pub private_key: String,
    #[clap(
        long,
        value_name = "SUBGRAPH",
        env = "REGISTRY_SUBGRAPH",
        help = "Subgraph endpoint to the Graphcast Registry",
        default_value = "https://api.thegraph.com/subgraphs/name/hopeyen/graphcast-registry-goerli"
    )]
    pub registry_subgraph: String,
    #[clap(
        long,
        value_name = "SUBGRAPH",
        env = "NETWORK_SUBGRAPH",
        help = "Subgraph endpoint to The Graph network subgraph",
        default_value = "https://gateway.testnet.thegraph.com/network"
    )]
    pub network_subgraph: String,
    #[clap(
        long,
        default_value = "testnet",
        value_name = "NAME",
        env = "GRAPHCAST_NETWORK",
        help = "Supported Graphcast networks: mainnet, testnet",
        possible_values = ["testnet", "mainnet"]
    )]
    pub graphcast_network: String,
    #[clap(
        long,
        value_name = "[TOPIC]",
        value_delimiter = ',',
        env = "TOPICS",
        help = "Comma separated static list of content topics to subscribe to"
    )]
    pub topics: Vec<String>,
    #[clap(
        long,
        min_values = 0,
        default_value = "120",
        value_name = "COLLECT_MESSAGE_DURATION",
        env = "COLLECT_MESSAGE_DURATION",
        help = "Set the minimum duration to wait for a topic message collection"
    )]
    pub collect_message_duration: i64,
    #[clap(
        long,
        value_name = "WAKU_HOST",
        help = "Host for the GraphQL HTTP server",
        env = "WAKU_HOST"
    )]
    pub waku_host: Option<String>,
    #[clap(
        long,
        value_name = "WAKU_PORT",
        help = "Port for the GraphQL HTTP server",
        env = "WAKU_PORT"
    )]
    pub waku_port: Option<String>,
    #[clap(
        long,
        value_name = "KEY",
        env = "WAKU_NODE_KEY",
        hide_env_values = true,
        help = "Private key to the Waku node id"
    )]
    pub waku_node_key: Option<String>,
    #[clap(
        long,
        value_name = "NODE_ADDRESSES",
        value_delimiter = ',',
        value_parser = Config::format_boot_node_addresses,
        help = "Comma separated static list of waku boot nodes to connect to",
        env = "BOOT_NODE_ADDRESSES"
    )]
    pub boot_node_addresses: Vec<Multiaddr>,
    #[clap(
        long,
        value_name = "WAKU_LOG_LEVEL",
        help = "Waku node logging configuration",
        env = "WAKU_LOG_LEVEL"
    )]
    pub waku_log_level: Option<String>,
    #[clap(
        long,
        value_name = "LOG_LEVEL",
        default_value = "info",
        help = "logging configurationt to set as RUST_LOG",
        env = "RUST_LOG"
    )]
    pub log_level: String,
    #[clap(
        long,
        value_name = "HOST:PORT",
        help = "Metrics endpoint to query/scrape",
        env = "METRICS_URL"
    )]
    pub metrics_url: Option<String>,
    #[clap(
        long,
        value_name = "SLACK_TOKEN",
        help = "Slack bot API token",
        env = "SLACK_TOKEN"
    )]
    pub slack_token: Option<String>,
    #[clap(
        long,
        value_name = "SLACK_WEBHOOK",
        help = "Webhook to the Slack bot App",
        env = "SLACK_WEBHOOK"
    )]
    pub slack_webhook: Option<String>,
}

impl Config {
    /// Parse config arguments
    pub fn args() -> Self {
        // TODO: load config file before parse (maybe add new level of subcommands)
        let config = Config::parse();
        std::env::set_var("RUST_LOG", config.log_level.clone());
        // Enables tracing under RUST_LOG variable
        init_tracing().expect("Could not set up global default subscriber for logger, check environmental variable `RUST_LOG` or the CLI input `log-level`");
        config
    }

    /// Load a configuration file if `opt.config` is set. If not, generate
    /// a config from the command line arguments in `opt`
    pub fn load(config: &Self) -> Result<Config, ConfigError> {
        if let Some(config) = &config.config_file {
            Self::from_file(config)
        } else {
            Err(ConfigError::ValidateInput(String::from(
                "Provide config file path",
            )))
        }
    }

    // Read a toml file to string
    pub fn from_file(path: &str) -> Result<Config, ConfigError> {
        let config_str = &read_to_string(path).map_err(ConfigError::ReadStr)?;
        let config: Config = toml::from_str(config_str).map_err(ConfigError::ReadToml)?;
        Ok(config)
    }

    /// Generate a JSON representation of the config.
    pub fn to_json(&self) -> Result<String, ConfigError> {
        serde_json::to_string_pretty(&self).map_err(ConfigError::GenerateJson)
    }

    /// Validate that private key as an Eth wallet
    fn parse_key(value: &str) -> Result<String, WalletError> {
        // The wallet can be stored instead of the original private key
        let wallet = value.parse::<LocalWallet>()?;
        let addr = graphcast_id_address(&wallet);
        info!("Resolved Graphcast id: {}", addr);
        Ok(String::from(value))
    }

    /// Helper function to parse supplied boot node addresses to multiaddress
    /// Defaults to an empty vec if it cannot find the 'BOOT_NODE_ADDRESSES' environment variable
    /// Multiple formats for defining the addresses list are supported, such as:
    /// 1. BOOT_NODE_ADDRESSES=addr
    /// 2. BOOT_NODE_ADDRESSES="addr1","addr2","addr3"
    /// 3. BOOT_NODE_ADDRESSES="addr1, addr2, addr3"
    /// 4. BOOT_NODE_ADDRESSES=addr1, addr2, addr3
    pub fn format_boot_node_addresses(address: &str) -> Result<Multiaddr, ConfigError> {
        let address = address.trim();
        if address.is_empty() {
            return Err(ConfigError::ValidateInput(String::from("Empty input")));
        }
        let address = if address.starts_with('"') && address.ends_with('"') {
            &address[1..address.len() - 1]
        } else {
            address
        };
        Multiaddr::from_str(address).map_err(|e| ConfigError::ValidateInput(e.to_string()))
    }

    /// Asynchronous validation to the configuration set ups
    pub async fn validate_set_up(&self) -> Result<&Self, ConfigError> {
        let wallet = self
            .private_key
            .parse::<LocalWallet>()
            .map_err(|e| ConfigError::ValidateInput(format!("Private key links to wallet: {e}")))?;
        let graphcast_id = graphcast_id_address(&wallet);
        // TODO: Implies invalidity for both graphcast id and registry, maybe map_err more specifically
        let indexer = query_registry_indexer(self.registry_subgraph.to_string(), graphcast_id)
            .await
            .map_err(|e| ConfigError::ValidateInput(format!("The registry subgraph did not contain an entry for the Graphcast ID to Indexer: {e}")))?;
        debug!("Resolved indexer identity: {}", indexer);

        let stake = query_network_subgraph(self.network_subgraph.to_string(), indexer)
            .await
            .map_err(|e| {
                ConfigError::ValidateInput(format!(
                    "The network subgraph must contain an entry for the Indexer stake: {e}"
                ))
            })?
            .indexer_stake();
        debug!("Resolved indexer stake: {}", stake);

        let _statuses = get_indexing_statuses(self.graph_node_endpoint.clone())
            .await
            .map_err(|e| {
                ConfigError::ValidateInput(format!(
                    "Graph node endpoint must be able to serve indexing statuses query: {e}"
                ))
            })?;
        Ok(self)
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
    Fantom,
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
            "fantom" => NetworkName::Fantom,
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
            NetworkName::Fantom => "fantom",
            NetworkName::Unknown => "unknown",
        };

        write!(f, "{name}")
    }
}

/// Maintained static list of supported Networks, the intervals target ~5minutes
/// depending on the blockchain average block processing time
pub static NETWORKS: Lazy<Vec<Network>> = Lazy::new(|| {
    vec![
        // Goerli (Ethereum Testnet): ~15 seconds
        Network {
            name: NetworkName::from_string("goerli"),
            interval: 20,
        },
        // Mainnet (Ethereum): ~10-12 seconds
        Network {
            name: NetworkName::from_string("mainnet"),
            interval: 30,
        },
        // Gnosis: ~5 seconds
        Network {
            name: NetworkName::from_string("gnosis"),
            interval: 60,
        },
        // Local test network
        Network {
            name: NetworkName::from_string("hardhat"),
            interval: 10,
        },
        // ArbitrumOne: ~0.5-1 second
        Network {
            name: NetworkName::from_string("arbitrum-one"),
            interval: 50,
        },
        // ArbitrumGoerli (Arbitrum Testnet): ~6 seconds
        Network {
            name: NetworkName::from_string("arbitrum-goerli"),
            interval: 60,
        },
        // Avalanche: ~3-5 seconds
        Network {
            name: NetworkName::from_string("avalanche"),
            interval: 60,
        },
        // Polygon: ~2 seconds
        Network {
            name: NetworkName::from_string("polygon"),
            interval: 150,
        },
        // Celo: ~5-10 seconds
        Network {
            name: NetworkName::from_string("celo"),
            interval: 30,
        },
        // Optimism: ~10-15 seconds
        Network {
            name: NetworkName::from_string("optimism"),
            interval: 20,
        },
        // Fantom: ~2-3 seconds
        Network {
            name: NetworkName::from_string("optimism"),
            interval: 100,
        },
    ]
});

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Validate the input: {0}")]
    ValidateInput(String),
    #[error("Generate JSON representation of the config file: {0}")]
    GenerateJson(serde_json::Error),
    #[error("Toml file error: {0}")]
    ReadToml(toml::de::Error),
    #[error("String parsing error: {0}")]
    ReadStr(std::io::Error),
    #[error("Unknown error: {0}")]
    Other(anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_boot_node_addresses_no_quotes() {
        let addresses = "addr1";
        let items = Config::format_boot_node_addresses(addresses);
        assert!(items.is_err());
        let err_msg = "Validate the input: invalid multiaddr".to_string();
        assert!(items.unwrap_err().to_string() == err_msg);
    }

    #[test]
    fn test_format_boot_node_addresses_single_item_good_addr() {
        let addresses =
            "/ip4/49.12.74.163/tcp/31900/p2p/16Uiu2HAm8doGEnyoGFn9sLN6XZkSzy81pxHT9MH6fgFmSnkG4unA";
        let items = Config::format_boot_node_addresses(addresses);
        assert!(items.is_ok());
    }

    #[test]
    fn test_format_boot_node_addresses_single_item_with_quotes() {
        let addresses = "\"/ip4/49.12.74.163/tcp/31900/p2p/16Uiu2HAm8doGEnyoGFn9sLN6XZkSzy81pxHT9MH6fgFmSnkG4unA\"";
        let items = Config::format_boot_node_addresses(addresses);
        assert!(items.is_ok());
    }

    #[test]
    fn test_parse_key_good_wallet() {
        let key = "0xdf57089febbacf7ba0bc227dafbffa9fc08a93fdc68e1e42411a14efcf23656e";
        let items = Config::parse_key(key);
        assert!(items.is_ok());
    }

    #[test]
    fn test_parse_key_bad_wallet() {
        let key = "hm";
        let items = Config::parse_key(key);
        assert!(items.is_err());
    }
}
