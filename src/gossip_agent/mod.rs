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
use ethers::signers::LocalWallet;
use prost::Message;
use std::collections::HashMap;
use std::error::Error;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use tracing::info;
use waku::{
    waku_set_event_callback, Multiaddr, Running, Signal, WakuContentTopic, WakuNodeHandle,
    WakuPubSubTopic,
};

use self::message_typing::GraphcastMessage;
use self::waku_handling::{
    build_content_topics, filter_peer_subscriptions, handle_signal, network_check, pubsub_topic,
    setup_node_handle,
};

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
    eth_node: String,
    provider: Provider<Http>,
    node_handle: WakuNodeHandle<Running>,
    /// gossip agent waku instance's pubsub topic
    pub pubsub_topic: Option<WakuPubSubTopic>,
    /// gossip agent waku instance's content topics
    pub content_topics: Vec<WakuContentTopic>,
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
    /// Inputs are utilized to construct different components of the Gossip agent:
    /// private_key resolves into ethereum wallet and indexer identity.
    /// radio_name is used as part of the content topic for the radio application
    /// subtopic optionally provided and used as the content topic identifier of the message subject,
    /// if not provided then they are generated based on indexer allocations
    /// Waku node address is set up by optionally providing a host and port, and an advertised address to be connected among the waku peers
    /// Advertised address can be any multiaddress that is self-describing and support addresses for any network protocol (tcp, udp, ip; tcp6, udp6, ip6 for IPv6)
    ///
    /// Content topics that the Radio subscribes to
    /// If we pass in `None` Graphcast will default to using the ipfs hashes of the subgraphs that the Indexer is allocating to.
    /// But in this case we will override it with something much more simple.
    /// # Examples
    ///
    /// ```ignore
    /// let agent = GossipAgent::new(
    ///     String::from("1231231231231231231231231231231231231231231231231231231231231230"),
    ///     String::from("https://goerli.infura.io/v3/api_key"),
    ///     "test_topic",
    ///     "https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test",
    ///     "https://gateway.testnet.thegraph.com/network",
    ///     Some(["some_subgraph_hash"].to_vec()),
    ///     Some(String::from("0.0.0.0")),
    ///     Some(String::from("60000")),
    ///     Some(String::from(/ip4/127.0.0.1/tcp/60000/p2p/16Uiu2YAmDEieEqD5dHSG85G8H51FUKByWoZx7byMy9AbMEgjd5iz")),
    /// )
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        private_key: String,
        eth_node: String,
        radio_name: &str,
        subtopics: Option<Vec<String>>,
        registry_subgraph: &str,
        network_subgraph: &str,
        waku_host: Option<String>,
        waku_port: Option<String>,
        waku_addr: Option<String>,
    ) -> Result<GossipAgent, Box<dyn Error>> {
        let wallet = private_key.parse::<LocalWallet>().unwrap();
        let provider: Provider<Http> = Provider::<Http>::try_from(eth_node.clone()).unwrap();
        let pubsub_topic: Option<WakuPubSubTopic> =
            pubsub_topic("0", &provider.get_chainid().await?.to_string());

        //Should we allow the setting of waku node host and port?
        let host = waku_host.as_deref();
        let port = waku_port.map(|y| y.parse().unwrap());
        let advertised_addr = waku_addr.map(|addr| {
            Multiaddr::from_str(&addr).expect(
                "Could not make format advertised address into a Multiaddress, do not advertise",
            )
        });
        let node_key = waku::SecretKey::from_str(&private_key).ok();

        let node_handle = setup_node_handle(&pubsub_topic, host, port, advertised_addr, node_key);

        // Filter subscriptions only if provided subtopic
        let content_topics = if let Some(topics) = subtopics {
            let content_topics = build_content_topics(radio_name, 0, &topics);
            let res = filter_peer_subscriptions(&node_handle, &pubsub_topic, &content_topics)
                .expect("Could not connect and subscribe to the subtopics");

            info!("Subtopic subscription: {:#?}", res);
            content_topics
        } else {
            [].to_vec()
        };

        Ok(GossipAgent {
            wallet,
            eth_node,
            provider,
            pubsub_topic,
            content_topics,
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

    pub fn print_subscriptions(&self) {
        info!("pubsub topic: {:#?}", &self.pubsub_topic);
        info!("content topics: {:#?}", &self.content_identifiers());
    }

    /// Find the subscribed content topic with an identifier
    /// Error if topic doesn't exist
    pub fn match_content_topic(
        &self,
        identifier: String,
    ) -> Result<&WakuContentTopic, Box<dyn Error>> {
        match self
            .content_topics
            .iter()
            .find(|&x| x.content_topic_name == identifier.clone())
        {
            Some(topic) => Ok(topic),
            _ => Err(anyhow::anyhow!(format!(
                "Did not match a content topic with identifier: {identifier}"
            )))?,
        }
    }

    /// Establish custom handler for incoming Waku messages
    pub fn register_handler<
        F: FnMut(Result<GraphcastMessage<T>, anyhow::Error>)
            + std::marker::Sync
            + std::marker::Send
            + 'static,
        T: Message + ethers::types::transaction::eip712::Eip712 + Default + Clone + 'static,
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
    pub async fn send_message<
        T: Message + ethers::types::transaction::eip712::Eip712 + Default + Clone + 'static,
    >(
        &self,
        identifier: String,
        block_number: u64,
        payload: Option<T>,
    ) -> Result<String, Box<dyn Error>> {
        let block_hash: String = format!(
            "{:#x}",
            self.provider
                .get_block(block_number)
                .await?
                .ok_or(AgentError::EmptyResponseError)?
                .hash
                .ok_or(AgentError::UnexpectedResponseError)?
        );
        let content_topic = self.match_content_topic(identifier.clone())?;

        // Check network before sending a message
        network_check(&self.node_handle)?;

        GraphcastMessage::build(
            &self.wallet,
            identifier,
            payload,
            block_number
                .try_into()
                .expect("Could not format block number"),
            block_hash,
        )
        .await?
        .send_to_waku(&self.node_handle, self.pubsub_topic.clone(), content_topic)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Query response is empty")]
    EmptyResponseError,
    #[error("Unexpected response format")]
    UnexpectedResponseError,
    #[error("Unknown error: {0}")]
    Other(anyhow::Error),
}
