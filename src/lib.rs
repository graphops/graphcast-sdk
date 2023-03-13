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

use config::BlockPointer;
use config::NetworkName;
use config::NETWORKS;
use ethers::signers::{Signer, Wallet};
use ethers_core::k256::ecdsa::SigningKey;
use graphcast_agent::message_typing::GraphcastMessage;

use once_cell::sync::OnceCell;
use prost::Message;

use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
};
use tokio::sync::Mutex as AsyncMutex;
use tracing::warn;
use tracing::{debug, subscriber::SetGlobalDefaultError};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::FmtSubscriber;
use url::{Host, Url};
use waku::WakuPubSubTopic;

pub mod config;
pub mod graphcast_agent;
pub mod graphql;
pub mod slack_bot;

type NoncesMap = HashMap<String, HashMap<String, i64>>;

/// Each radio persist Nonces from sub-topic messages differentiating sender
pub static NONCES: OnceCell<Arc<Mutex<NoncesMap>>> = OnceCell::new();

/// Returns Graphcast application domain name
pub fn app_name() -> Cow<'static, str> {
    Cow::from("graphcast")
}

/// Returns hardcoded DNS Url to a discoverable ENR tree that should be used to retrieve boot nodes
pub fn discovery_url(pubsub_topic: &WakuPubSubTopic) -> Result<Url, url::ParseError> {
    let enr_url = config_env_var("ENR_URL").unwrap_or_else(|_| {
        if pubsub_topic.topic_name == "graphcast-v0-mainnet"{
            "enrtree://APDKVCM3Q7TLTBD2FXKMXNIOIDPQRXNNI4ZXKEQLOWAFO3BZXZM3C@mainnet.bootnodes.graphcast.xyz"
            .to_string()
        }else {
            "enrtree://AOADZWXPAJ56TIXA74PV7VJP356QNBIKUPRKR676BBOOELU5XDDKM@testnet.bootnodes.graphcast.xyz"
                .to_string()
        }
    });
    debug!("ENRtree url used for DNS discovery: {}", enr_url);

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

/// Sets up tracing, allows log level to be set from the environment variables
pub fn init_tracing() -> Result<(), SetGlobalDefaultError> {
    let filter = EnvFilter::from_default_env();
    // let level_filter = filter.max_level_hint().unwrap_or(Level::ERROR.into());

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(filter)
        // .with_max_level(level_filter)
        .with_ansi(true)
        .with_target(true)
        .with_level(true)
        .with_line_number(true)
        .pretty()
        .finish();
    tracing::subscriber::set_global_default(subscriber)
}

/* Blocks operation */
/// Filters for the first message of a particular identifier by block number
/// get the timestamp it was received from and add the collection duration to
/// return the time for which message comparisons should be triggered
pub async fn comparison_trigger<
    T: Message + ethers::types::transaction::eip712::Eip712 + Default + Clone + 'static,
>(
    messages: Arc<AsyncMutex<Vec<GraphcastMessage<T>>>>,
    identifier: String,
    collect_duration: i64,
) -> (u64, i64) {
    let messages = AsyncMutex::new(messages.lock().await);
    let msgs = messages.lock().await;
    // Filter the messages to get only those that have the matching identifier:
    let matched_msgs = msgs
        .iter()
        .filter(|message| message.identifier == identifier);
    // Use min_by_key to get the message with the minimum value of (block_number, nonce), and add collect_duration to its nonce value to get the trigger time
    let msg_trigger_time = matched_msgs
        .min_by_key(|msg| (msg.block_number, msg.nonce))
        .map(|message| (message.block_number, message.nonce + collect_duration));

    // If no matching message is found, return (0, i64::MAX) as the trigger
    msg_trigger_time.unwrap_or((0, i64::MAX))
}

/// This function determines the relevant block to send the message for, depending on the network chainhead block
/// and its pre-configured examination frequency
pub fn determine_message_block(
    network_chainhead_blocks: &HashMap<NetworkName, BlockPointer>,
    network_name: NetworkName,
) -> Result<u64, NetworkBlockError> {
    // Get the pre-configured examination frequency of the network
    let examination_frequency = NETWORKS
        .iter()
        .find(|n| n.name.to_string() == network_name.to_string()).map(|n| n.interval)
        .ok_or({
            let err_msg = format!("Subgraph is indexing an unsupported network {network_name}, please report an issue on https://github.com/graphops/graphcast-rs");
            warn!(err_msg);
            NetworkBlockError::UnsupportedNetwork(err_msg)
        })?;

    // Calculate the relevant block for the message
    match network_chainhead_blocks.get(&network_name) {
        Some(BlockPointer { hash: _, number }) => Ok(number - number % examination_frequency),
        None => {
            let err_msg = format!(
                "Could not get the chainhead block number on network {network_name} for determining the message's relevant block",
            );
            warn!(err_msg);
            Err(NetworkBlockError::FailedStatus(err_msg))
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NetworkBlockError {
    #[error("Unsupported network: {0}")]
    UnsupportedNetwork(String),
    #[error("Failed to query syncing status of the network: {0}")]
    FailedStatus(String),
    #[error("Cannot get network's block information: {0}")]
    Other(anyhow::Error),
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
