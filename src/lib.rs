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

use ethers::signers::{
    coins_bip39::English, LocalWallet, MnemonicBuilder, Signer, Wallet, WalletError,
};
use ethers_core::k256::ecdsa::SigningKey;
use graphcast_agent::message_typing::{BuildMessageError, GraphcastMessage};
use graphql::{
    client_graph_account::query_graph_account, client_network::query_network_subgraph, QueryError,
};
use networks::{NetworkName, NETWORKS};

use once_cell::sync::OnceCell;
use prost::Message;
use serde::{Deserialize, Serialize};

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

use crate::{graphcast_agent::ConfigError, graphql::client_registry::query_registry};

pub mod bots;
pub mod callbook;
pub mod graphcast_agent;
pub mod graphql;
pub mod networks;

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
    debug!(ENR_Tree = enr_url, "DNS discovery");

    Url::parse(&enr_url)
}

pub fn cf_nameserver() -> Host {
    Host::Domain("konnor.ns.cloudflare.com".to_string())
}

/// Attempt to read environmental variable
pub fn config_env_var(name: &str) -> Result<String, String> {
    env::var(name).map_err(|e| format!("{name}: {e}"))
}

/// Build Wallet from Private key or Mnemonic
pub fn build_wallet(value: &str) -> Result<Wallet<SigningKey>, WalletError> {
    value
        .parse::<LocalWallet>()
        .or(MnemonicBuilder::<English>::default().phrase(value).build())
}

/// Get wallet public address to String
pub fn wallet_address(wallet: &Wallet<SigningKey>) -> String {
    format!("{:?}", wallet.address())
}

/// Sets up tracing, allows log level to be set from the environment variables
pub fn init_tracing(format: String) -> Result<(), SetGlobalDefaultError> {
    let filter = EnvFilter::from_default_env();

    let subscriber_builder: tracing_subscriber::fmt::SubscriberBuilder<
        tracing_subscriber::fmt::format::DefaultFields,
        tracing_subscriber::fmt::format::Format,
        EnvFilter,
    > = FmtSubscriber::builder().with_env_filter(filter);

    match format.as_str() {
        "json" => tracing::subscriber::set_global_default(subscriber_builder.json().finish()),
        "full" => tracing::subscriber::set_global_default(subscriber_builder.finish()),
        "compact" => tracing::subscriber::set_global_default(subscriber_builder.compact().finish()),
        _ => tracing::subscriber::set_global_default(
            subscriber_builder.with_ansi(true).pretty().finish(),
        ),
    }
}

/* Blocks operation */
/// Filters for the first message of a particular identifier by block number
/// get the timestamp it was received from and add the collection duration to
/// return the time for which message comparisons should be triggered
pub async fn comparison_trigger<
    T: Message
        + ethers::types::transaction::eip712::Eip712
        + Default
        + Clone
        + 'static
        + async_graphql::OutputType,
>(
    messages: Arc<AsyncMutex<Vec<GraphcastMessage<T>>>>,
    identifier: &str,
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
    let examination_frequency = match NETWORKS.iter().find(|n| n.name == network_name) {
        Some(n) => n.interval,
        None => {
            let err_msg = format!("Subgraph is indexing an unsupported network {network_name}, please report an issue on https://github.com/graphops/graphcast-rs");
            warn!(err_msg);
            return Err(NetworkBlockError::UnsupportedNetwork(err_msg));
        }
    };

    // Calculate the relevant block for the message
    match network_chainhead_blocks.get(&network_name) {
        Some(BlockPointer { hash: _, number }) => Ok(number - number % examination_frequency),
        None => {
            let err_msg = format!(
                "Could not get the chainhead block number on network {network_name} for determining the message's relevant block, check if graph node has a deployment indexing the network",
            );
            warn!(err_msg);
            Err(NetworkBlockError::FailedStatus(err_msg))
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct NetworkPointer {
    pub network: String,
    pub block: BlockPointer,
}

pub struct BlockClock {
    pub current_block: u64,
    pub compare_block: u64,
}

/// Struct for a block pointer
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct BlockPointer {
    pub number: u64,
    pub hash: String,
}

impl BlockPointer {
    pub fn new(number: u64, hash: String) -> Self {
        BlockPointer { number, hash }
    }
}

/// Account information to keep graphcast agent signer address,
/// and its correseponding Graph Account. `agent` address can be validated as either
/// a graphcast_id, an indexer operator, or an indexer. `account` address takes the field `graph_account` from a generic `GraphcastMessage` and gets verified through Graphcast registry and/or Graph network subgraph, through a locally configured `IdentityValidation` mechanism.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub agent: String,
    pub account: String,
}

impl Account {
    /// Create an account with agent: a graphcast_id, an indexer operator, or an indexer
    /// account: graphcast registered account (currently limited to indexer address), Graph account, Indexer
    pub fn new(agent_addr: String, graph_account: String) -> Self {
        Account {
            agent: agent_addr,
            account: graph_account,
        }
    }

    /// Get Graphcast agent address
    pub fn agent_address(&self) -> &str {
        &self.agent
    }

    /// Get agent's representing graph account
    pub fn account(&self) -> &str {
        &self.account
    }

    /// Check for sender's registration at Graphcast (registered at graphcast Registry)
    pub async fn account_from_registry(
        &self,
        registry_subgraph: &str,
    ) -> Result<Account, QueryError> {
        let registered_address = query_registry(registry_subgraph, self.agent_address()).await?;

        Ok(Account::new(
            self.agent_address().to_string(),
            registered_address,
        ))
    }

    /// Check for sender's registration at Graph Network
    pub async fn account_from_network(
        &self,
        network_subgraph: &str,
    ) -> Result<Account, QueryError> {
        let matched_account =
            query_graph_account(network_subgraph, self.agent_address(), self.account()).await?;
        Ok(matched_account)
    }

    pub async fn valid_indexer(&self, network_subgraph: &str) -> Result<(), BuildMessageError> {
        if query_network_subgraph(network_subgraph, self.account())
            .await
            .map_err(BuildMessageError::FieldDerivations)?
            .stake_satisfy_requirement()
        {
            Ok(())
        } else {
            Err(BuildMessageError::InvalidFields(anyhow::anyhow!(
                "Sender stake is less than the minimum requirement, drop message"
            )))
        }
    }
}

/// Struct for a block pointer
#[derive(Clone, PartialEq, Debug)]
pub struct GraphcastIdentity {
    wallet: LocalWallet,
    graphcast_id: String,
    // indexer address but will include other graph accounts
    graph_account: String,
}

impl GraphcastIdentity {
    /// Function to create a Graphcast Identity
    /// Which should include an Graphcast Agent key (graphcast_id, indexer_operator, indexer)
    /// mapped to Graph Network Graph Account Identity
    pub async fn new(agent_key: String, graph_account: String) -> Result<Self, ConfigError> {
        let wallet = build_wallet(&agent_key).map_err(|e| {
            ConfigError::ValidateInput(format!(
                "Invalid key to wallet, use private key or mnemonic: {e}"
            ))
        })?;
        let graphcast_id = wallet_address(&wallet);
        Ok(GraphcastIdentity {
            wallet,
            graphcast_id,
            graph_account: graph_account.to_lowercase(),
        })
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
}
