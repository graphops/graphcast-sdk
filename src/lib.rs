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
use graphcast_agent::message_typing::{BuildMessageError, IdentityValidation};
use graphql::{
    client_graph_account::{query_graph_account, subgraph_hash_by_id},
    client_network::query_network_subgraph,
    QueryError,
};
use networks::{NetworkName, NETWORKS};

use once_cell::sync::OnceCell;

use serde::{Deserialize, Serialize};

use std::{
    borrow::Cow,
    collections::HashMap,
    env, fmt,
    sync::{Arc, Mutex},
};

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

//TODO: maybe blocks operation should be on a radio level
/* Blocks operation */
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
            let err_msg = format!("Subgraph is indexing an unsupported network {network_name}, please report an issue on https://github.com/graphops/graphcast-sdk");
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

    pub async fn valid_indexer(&self, network_subgraph: &str) -> Result<bool, BuildMessageError> {
        Ok(query_network_subgraph(network_subgraph, self.account())
            .await
            .map_err(BuildMessageError::FieldDerivations)?
            .stake_satisfy_requirement())
    }

    /// Check if the account owns a subgraph id
    pub async fn valid_owner(
        &self,
        network_subgraph: &str,
        subgraph_hash: &str,
        subgraph_id: &str,
    ) -> Result<bool, BuildMessageError> {
        let subgraph_hashes = subgraph_hash_by_id(network_subgraph, self.account(), subgraph_id)
            .await
            .map_err(BuildMessageError::FieldDerivations)?;
        Ok(!subgraph_hashes.contains(&subgraph_hash.to_string()))
    }

    /// Based on id_validation mechanism, perform the corresponding check between the
    /// message sender versus the representing graph_account.
    ///
    /// Currently do not verify subgraph ownership here, which should be used `IdentityValidation::SubgraphStaker`
    pub async fn verify(
        &self,
        network_subgraph: &str,
        registry_subgraph: &str,
        id_validation: &IdentityValidation,
    ) -> Result<Account, BuildMessageError> {
        let verified_account: Account = match id_validation {
            IdentityValidation::NoCheck | IdentityValidation::ValidAddress => self.clone(),
            IdentityValidation::GraphcastRegistered => {
                // Simply check if the message signer is registered at Graphcast Registry, make no validation on Graph Account field
                self.account_from_registry(registry_subgraph)
                    .await
                    .map_err(BuildMessageError::FieldDerivations)?
            }
            IdentityValidation::GraphNetworkAccount => {
                // allow any Graph account matched with message signer and the self-claimed graph account
                self.account_from_network(network_subgraph)
                    .await
                    .map_err(BuildMessageError::FieldDerivations)?
            }
            IdentityValidation::RegisteredIndexer => self
                .account_from_registry(registry_subgraph)
                .await
                .map_err(BuildMessageError::FieldDerivations)?,
            IdentityValidation::Indexer | IdentityValidation::SubgraphStaker => {
                match self.account_from_registry(registry_subgraph).await {
                    Ok(a) => a,
                    Err(e) => {
                        debug!(
                            e = tracing::field::debug(&e),
                            account = tracing::field::debug(&self),
                            "Signer is not registered at Graphcast Registry. Check Graph Network"
                        );
                        self.account_from_network(network_subgraph)
                            .await
                            .map_err(BuildMessageError::FieldDerivations)?
                    }
                }
            }
        };
        // Require account info to be consistent from validation mechanism
        if verified_account.account != self.account {
            return Err(BuildMessageError::InvalidFields(anyhow::anyhow!(
                "Verified account is not the one claimed by the message, drop message"
            )));
        };

        // Indexer check for indexer validating mechanisms
        if ((id_validation == &IdentityValidation::RegisteredIndexer)
            | (id_validation == &IdentityValidation::Indexer))
            && !(verified_account.valid_indexer(network_subgraph).await?)
        {
            return Err(BuildMessageError::InvalidFields(anyhow::anyhow!(format!(
                "Verified account failed indexer requirement. Verified account: {:#?}",
                verified_account
            ))));
        };

        // SubgraphStaker check for subgraph owner ship is done in Radio Level, triggered
        // when message field provides detailed information

        Ok(verified_account)
    }
}

/// Struct for a block pointer
#[derive(Clone, PartialEq, Debug)]
pub struct GraphcastIdentity {
    wallet: LocalWallet,
    pub graphcast_id: String,
    // indexer address but will include other graph accounts
    pub graph_account: String,
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

#[derive(clap::ValueEnum, Clone, Debug, Serialize, Deserialize, Default)]
pub enum LogFormat {
    Compact,
    #[default]
    Pretty,
    Json,
    Full,
}

impl fmt::Display for LogFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogFormat::Compact => write!(f, "compact"),
            LogFormat::Pretty => write!(f, "pretty"),
            LogFormat::Json => write!(f, "json"),
            LogFormat::Full => write!(f, "full"),
        }
    }
}

#[derive(clap::ValueEnum, Clone, Debug, Serialize, Deserialize, Default)]
pub enum GraphcastNetworkName {
    #[default]
    Testnet,
    Mainnet,
}

impl fmt::Display for GraphcastNetworkName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphcastNetworkName::Testnet => write!(f, "testnet"),
            GraphcastNetworkName::Mainnet => write!(f, "mainnet"),
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
