//! Type for representing a Gossip agent for interacting with Graphcast.
//!
//! A "GossipAgent" has access to
//! - Gossip operator wallet: resolve Graph Account identity
//! - Ethereum node provider endpoint: provider access
//! - Waku Node Instance: interact with the gossip network
//! - Pubsub and Content filter topics: interaction configurations
//!
//! Gossip agent shall be able to construct, send, receive, validate, and attest
//! Graphcast messages regardless of specific radio use cases
//!
use ethers::providers::{Http, Middleware, Provider};
use ethers::signers::{LocalWallet, Signer};
use ethers::types::Block;

use data_encoding::BASE32;
use secp256k1::{PublicKey, Secp256k1, SecretKey as SKey};
use std::collections::HashMap;
use std::error::Error;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use waku::{waku_set_event_callback, Running, Signal, WakuContentTopic, WakuNodeHandle};

use self::message_typing::GraphcastMessage;
use self::waku_handling::{build_content_topics, handle_signal, pubsub_topic, setup_node_handle};
use crate::graphql::client_network::query_network_subgraph;
use crate::graphql::client_registry::query_registry_indexer;
use crate::NoncesMap;

pub mod message_typing;
pub mod waku_handling;

/// A constant defining a message expiration limit.
pub const MSG_REPLAY_LIMIT: i64 = 3_600_000;
/// A constant defining the goerli registry subgraph endpoint.
pub const REGISTRY_SUBGRAPH: &str =
    "https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test";
/// A constant defining the goerli network subgraph endpoint.
pub const NETWORK_SUBGRAPH: &str = "https://gateway.testnet.thegraph.com/network";

/// Find the subscribed content topic with an identifier
/// Error if topic doesn't exist
pub fn match_content_topic(
    identifier: String,
    content_topics: &[WakuContentTopic],
) -> Result<&WakuContentTopic, Box<dyn Error>> {
    match content_topics
        .iter()
        .find(|&x| x.content_topic_name == identifier.clone())
    {
        Some(topic) => Ok(topic),
        _ => Err(anyhow::anyhow!(format!(
            "Did not match a content topic with identifier: {}",
            identifier
        )))?,
    }
}

/// A gossip agent representation
pub struct GossipAgent {
    /// Gossip operator's wallet, used to sign messages
    pub wallet: LocalWallet,
    /// Gossip operator's indexer address
    pub indexer_address: String,
    eth_node: String,
    provider: Provider<Http>,
    /// Gossip operator's indexer allocations, used for default topic generation
    pub indexer_allocations: Vec<String>,
    node_handle: WakuNodeHandle<Running>,
    /// gossip agent waku instance's content topics
    content_topics: Vec<WakuContentTopic>,
    /// Nonces map for caching sender nonces in each subtopic
    pub nonces: Arc<Mutex<NoncesMap>>,
    /// A constant defining the goerli registry subgraph endpoint.
    pub registry_subgraph: String,
    /// A constant defining the goerli network subgraph endpoint.
    pub network_subgraph: String,
}

impl GossipAgent {
    /// Construct a new gossip agent
    ///
    /// Private key resolves into wallet and indexer identity.
    /// Topic is generated based on indexer allocations with waku node set up included
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        private_key: String,
        eth_node: String,
        radio_name: &str,
        registry_subgraph: &str,
        network_subgraph: &str,
        subtopics: Option<Vec<&str>>,
        waku_host: Option<String>,
        waku_port: Option<String>,
    ) -> Result<GossipAgent, Box<dyn Error>> {
        let wallet = private_key.parse::<LocalWallet>().unwrap();
        let provider: Provider<Http> = Provider::<Http>::try_from(eth_node.clone()).unwrap();
        let indexer_address = query_registry_indexer(
            registry_subgraph.to_string(),
            format!("{:?}", wallet.address()),
        )
        .await?;

        // TODO: Factor out custom topic generation query
        let indexer_allocations =
            query_network_subgraph(network_subgraph.to_string(), indexer_address.clone())
                .await?
                .indexer_allocations();
        let subtopics = subtopics.unwrap_or_else(|| {
            indexer_allocations
                .iter()
                .map(|s| &**s)
                .collect::<Vec<&str>>()
        });

        let content_topics = build_content_topics(radio_name, 0, &subtopics);
        //Should we allow the setting of waku node host and port?
        let host = waku_host.as_deref();
        let port = waku_port.map(|y| y.parse().unwrap());
        let node_key = waku::SecretKey::from_str(&private_key).ok();

        // Print out base32 encoded public key, use in discovery URL TXT field
        // Can be moved to only print for boot nodes
        let secret_key = SKey::from_str(&private_key);
        if let Ok(sk) = secret_key {
            let secp = Secp256k1::new();
            let public_key = PublicKey::from_secret_key(&secp, &sk);
            let pk = public_key.serialize();
            let base32_encoded_key = BASE32.encode(&pk);
            println!(
                "Base32 encoded 32byte Public key: {:#?}",
                base32_encoded_key
            );
        }

        let node_handle = setup_node_handle(&content_topics, host, port, node_key);
        Ok(GossipAgent {
            wallet,
            eth_node,
            provider,
            indexer_address,
            content_topics,
            indexer_allocations,
            node_handle,
            nonces: Arc::new(Mutex::new(HashMap::new())),
            registry_subgraph: registry_subgraph.to_string(),
            network_subgraph: network_subgraph.to_string(),
        })
    }

    /// Get identifiers of Radio content topics
    pub fn content_identifiers(&self) -> Vec<String> {
        self.content_topics
            .clone()
            .into_iter()
            .map(|topic| topic.content_topic_name.into_owned())
            .collect()
    }

    /// Establish custom handler for incoming Waku messages
    pub fn register_handler<
        F: FnMut(Result<GraphcastMessage, anyhow::Error>)
            + std::marker::Sync
            + std::marker::Send
            + 'static,
    >(
        &'static self,
        radio_handler_mutex: Arc<Mutex<F>>,
    ) {
        let provider: Provider<Http> = Provider::<Http>::try_from(&self.eth_node.clone()).unwrap();
        let handle_async = move |signal: Signal| {
            let rt = Runtime::new().unwrap();

            rt.block_on(async {
                let msg = handle_signal(
                    &provider,
                    signal,
                    &self.nonces,
                    &self.content_topics,
                    &self.registry_subgraph,
                    &self.network_subgraph,
                )
                .await;
                let mut radio_handler = radio_handler_mutex.lock().unwrap();
                radio_handler(msg);
            });
        };
        waku_set_event_callback(handle_async);
    }

    /// For each topic, construct with custom write function and send
    pub async fn send_message(
        &self,
        identifier: String,
        block_number: u64,
        content: String,
    ) -> Result<String, Box<dyn Error>> {
        let block: Block<_> = self
            .provider
            .get_block(block_number)
            .await
            .expect("Failed to query block from node provider based on block number")
            .expect("Node Provider returned None for the queried block");
        let block_hash = format!("{:#x}", block.hash.unwrap());
        let content_topic = match_content_topic(identifier.clone(), &self.content_topics)?;

        GraphcastMessage::build(
            &self.wallet,
            identifier,
            content,
            block_number.try_into().unwrap(),
            block_hash.to_string(),
        )
        .await?
        .send_to_waku(&self.node_handle, pubsub_topic("1"), content_topic)
    }
}
