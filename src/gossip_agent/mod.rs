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

use anyhow::anyhow;
use ethers::providers::{Http, Middleware, Provider};
use ethers::signers::{LocalWallet, Signer};
use ethers::types::Block;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::{borrow::Cow, error::Error};
use tokio::runtime::Runtime;
use waku::{
    waku_set_event_callback, Encoding, Running, Signal, WakuContentTopic, WakuNodeHandle,
    WakuPubSubTopic,
};

use self::message_typing::GraphcastMessage;
use self::waku_handling::{generate_pubsub_topics, handle_signal, setup_node_handle};
use crate::graphql::client_network::query_network_subgraph;
use crate::graphql::client_registry::query_registry_indexer;
use crate::{NoncesMap, Sender};

pub mod message_typing;
pub mod waku_handling;

/// A constant defining a message expiration limit.
pub const MSG_REPLAY_LIMIT: i64 = 3_600_000;
/// A constant defining the goerli registry subgraph endpoint.
pub const REGISTRY_SUBGRAPH: &str =
    "https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test";
/// A constant defining the goerli network subgraph endpoint.
pub const NETWORK_SUBGRAPH: &str = "https://gateway.testnet.thegraph.com/network";

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
    content_topic: WakuContentTopic,
    node_handle: WakuNodeHandle<Running>,
    /// Nonces map for caching sender nonces in each subtopic
    pub nonces: Arc<Mutex<NoncesMap>>,
}

impl GossipAgent {
    /// Construct a new gossip agent
    ///
    /// Private key resolves into wallet and indexer identity.
    /// Topic is generated based on indexer allocations with waku node set up included
    pub async fn new(
        private_key: String,
        eth_node: String,
        radio_name: &str,
    ) -> Result<GossipAgent, Box<dyn Error>> {
        let wallet = private_key.parse::<LocalWallet>().unwrap();
        let provider: Provider<Http> = Provider::<Http>::try_from(eth_node.clone()).unwrap();
        let content_topic: WakuContentTopic = WakuContentTopic {
            application_name: crate::app_name(),
            version: 0,
            content_topic_name: Cow::from(radio_name.to_string()),
            encoding: Encoding::Proto,
        };

        let indexer_address = query_registry_indexer(
            REGISTRY_SUBGRAPH.to_string(),
            format!("{:?}", wallet.address()),
        )
        .await?;

        // TODO: Factor out custom topic generation query
        let indexer_allocations =
            query_network_subgraph(NETWORK_SUBGRAPH.to_string(), indexer_address.clone())
                .await?
                .indexer_allocations();
        let pubsub_topics: Vec<Option<WakuPubSubTopic>> =
            generate_pubsub_topics(radio_name, &indexer_allocations);
        let node_handle = setup_node_handle(&pubsub_topics);

        Ok(GossipAgent {
            wallet,
            eth_node,
            provider,
            indexer_address,
            // pubsub_topics,
            indexer_allocations,
            content_topic,
            node_handle,
            nonces: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    // Note: Would be nice to factor out provider with eth_node, maybe impl Copy trait
    /// Establish custom handler for incoming Waku messages
    pub fn register_handler<F: FnMut(Result<(Sender, GraphcastMessage), anyhow::Error>) + std::marker::Sync + std::marker::Send + 'static>(
        &'static self,
        radio_handler_mutex: Arc<Mutex<F>>,
    ) {
        let provider: Provider<Http> = Provider::<Http>::try_from(&self.eth_node.clone()).unwrap();
        let handle_async = move |signal: Signal| {
            let rt = Runtime::new().unwrap();

            rt.block_on(async {
                let msg = handle_signal(&provider, signal, &self.nonces).await;
                let mut radio_handler = radio_handler_mutex.lock().unwrap();
                radio_handler(msg);
            });
        };
        waku_set_event_callback(handle_async);
    }

    /// For each topic, construct with custom write function and send
    pub async fn send_message(
        &self,
        topic: Option<WakuPubSubTopic>,
        block_number: u64,
        content: String,
    ) -> Result<String, Box<dyn Error>> {
        let block: Block<_> = self
            .provider
            .get_block(block_number)
            .await
            .unwrap()
            .unwrap();
        let block_hash = format!("{:#x}", block.hash.unwrap());
        let topic_title = match topic.clone() {
            Some(x) => x.topic_name,
            None => return Err(anyhow!("Could not parse topic title"))?,
        };
        let identifier: &str = topic_title.split('-').collect::<Vec<_>>()[3];

        GraphcastMessage::build(
            &self.wallet,
            identifier.to_string(),
            content,
            block_number.try_into().unwrap(),
            block_hash.to_string(),
        )
        .await?
        .send_to_waku(&self.node_handle, topic.clone(), self.content_topic.clone())
    }
}
