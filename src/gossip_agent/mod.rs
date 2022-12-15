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
use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use waku::{waku_set_event_callback, Running, Signal, WakuContentTopic, WakuNodeHandle};

use self::message_typing::GraphcastMessage;
use self::waku_handling::{
    handle_signal, pubsub_topic, setup_node_handle,
};
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
    // #[allow(dead_code)]
    // pubsub_topics: Vec<Option<WakuPubSubTopic>>,
    // content_topics: Vec<WakuContentTopic>,
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
        _radio_name: &str,
    ) -> Result<GossipAgent, Box<dyn Error>> {
        let wallet = private_key.parse::<LocalWallet>().unwrap();
        let provider: Provider<Http> = Provider::<Http>::try_from(eth_node.clone()).unwrap();
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
        // let subtopics = indexer_allocations
        //     .iter()
        //     .map(|s| &**s)
        //     .collect::<Vec<&str>>();
        let node_handle = setup_node_handle();
        // let _content_topics = generate_content_topics(radio_name, 0, &subtopics);
        Ok(GossipAgent {
            wallet,
            eth_node,
            provider,
            indexer_address,
            // pubsub_topics,
            // content_topics,
            indexer_allocations,
            node_handle,
            nonces: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    //TODO: Factor out handler
    /// Establish custom handler for incoming Waku messages
    pub fn message_handler(&'static self) {
        let provider: Provider<Http> = Provider::<Http>::try_from(&self.eth_node.clone()).unwrap();
        let handle_async = move |signal: Signal| {
            let rt = Runtime::new().unwrap();

            rt.block_on(async {
                handle_signal(&provider, &self.nonces, signal).await;
            });
        };
        waku_set_event_callback(handle_async);
    }

    /// Construct a Graphcast message and send to the Waku Relay network with custom topic
    pub async fn gossip_message(
        &self,
        content_topic: &WakuContentTopic,
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
        let identifier = &content_topic.content_topic_name;

        GraphcastMessage::build(
            &self.wallet,
            identifier.to_string(),
            content,
            block_number.try_into().unwrap(),
            block_hash.to_string(),
        )
        .await?
        .send_to_waku(&self.node_handle, pubsub_topic("1"), content_topic)
    }
}
