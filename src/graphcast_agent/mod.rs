//! Type for representing a Graphcast agent for interacting with Graphcast.
//!
//! A "GraphcastAgent" has access to
//! - GraphcastID wallet: resolve Graph Account identity
//! - Ethereum node provider endpoint: provider access
//! - Waku Node Instance: interact with the Graphcast network
//! - Pubsub and Content filter topics: interaction configurations
//!
//! Graphcast agent shall be able to construct, send, receive, validate, and attest
//! Graphcast messages regardless of specific radio use cases
//!

use ethers::signers::{LocalWallet, WalletError};
use prost::Message;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::Mutex;
use tracing::{debug, info};
use url::ParseError;
use waku::{
    waku_set_event_callback, Multiaddr, Running, Signal, WakuContentTopic, WakuNodeHandle,
    WakuPubSubTopic,
};

use self::message_typing::GraphcastMessage;
use self::waku_handling::{
    build_content_topics, filter_peer_subscriptions, handle_signal, network_check, pubsub_topic,
    setup_node_handle, WakuHandlingError,
};

use crate::graphcast_agent::waku_handling::unsubscribe_peer;
use crate::graphql::client_graph_node::query_graph_node_network_block_hash;
use crate::graphql::QueryError;
use crate::{NetworkName, NoncesMap};

pub mod message_typing;
pub mod waku_handling;

/// A constant defining a message expiration limit.
pub const MSG_REPLAY_LIMIT: i64 = 3_600_000;
/// A constant defining the goerli registry subgraph endpoint.
pub const REGISTRY_SUBGRAPH: &str =
    "https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test";
/// A constant defining the goerli network subgraph endpoint.
pub const NETWORK_SUBGRAPH: &str = "https://gateway.testnet.thegraph.com/network";

/// A Graphcast agent representation
pub struct GraphcastAgent {
    /// GraphcastID's wallet, used to sign messages
    pub wallet: LocalWallet,
    node_handle: WakuNodeHandle<Running>,
    /// Graphcast agent waku instance's radio application
    pub radio_name: &'static str,
    /// Graphcast agent waku instance's pubsub topic
    pub pubsub_topic: WakuPubSubTopic,
    /// Graphcast agent waku instance's content topics
    pub content_topics: Arc<Mutex<Vec<WakuContentTopic>>>,
    /// Nonces map for caching sender nonces in each subtopic
    pub nonces: Arc<Mutex<NoncesMap>>,
    /// A constant defining Graphcast registry subgraph endpoint
    pub registry_subgraph: String,
    /// A constant defining The Graph network subgraph endpoint
    pub network_subgraph: String,
    /// A constant defining the graph node endpoint
    pub graph_node_endpoint: String,
}

impl GraphcastAgent {
    /// Construct a new Graphcast agent
    ///
    /// Inputs are utilized to construct different components of the Graphcast agent:
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
    /// let agent = GraphcastAgent::new(
    ///     String::from("1231231231231231231231231231231231231231231231231231231231231230"),
    ///     String::from("https://goerli.infura.io/v3/api_key"),
    ///     "test_radio",
    ///     "https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test",
    ///     "https://gateway.testnet.thegraph.com/network",
    ///     vec![String::from("/ip4/127.0.0.1/tcp/60000/p2p/16Uiu2YAmDEieEqD5dHSG85G8H51FUKByWoZx7byMy9AbMEgjd5iz")],
    ///     Some("test_namespace_in_pubsub_topic"),
    ///     Some(["some_subgraph_hash"].to_vec()),
    ///     String::from("waku_node_key_can_be_same_as_private1231231231231231231231231230"),
    ///     Some(String::from("0.0.0.0")),
    ///     Some(String::from("60000")),
    ///     Some(String::from(/ip4/321.1.1.2/tcp/60001/p2p/16Uiu2YAmDEieEqD5dHSG85G8H51FUKByWoZx7byMysomeoneelse")),
    /// )
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        private_key: String,
        radio_name: &'static str,
        registry_subgraph: &str,
        network_subgraph: &str,
        graph_node_endpoint: &str,
        boot_node_addresses: Vec<String>,
        graphcast_namespace: Option<&str>,
        subtopics: Vec<String>,
        waku_node_key: Option<String>,
        waku_host: Option<String>,
        waku_port: Option<String>,
        waku_addr: Option<String>,
    ) -> Result<GraphcastAgent, GraphcastAgentError> {
        let wallet = private_key.parse::<LocalWallet>()?;
        let pubsub_topic: WakuPubSubTopic = pubsub_topic(graphcast_namespace);

        //Should we allow the setting of waku node host and port?
        let host = waku_host.as_deref();
        let port = waku_port.as_deref();

        let advertised_addr = waku_addr.and_then(|a| Multiaddr::from_str(&a).ok());
        let node_key = waku_node_key.and_then(|key| waku::SecretKey::from_str(&key).ok());

        let node_handle = setup_node_handle(
            boot_node_addresses,
            &pubsub_topic,
            host,
            port,
            advertised_addr,
            node_key,
        )
        .map_err(GraphcastAgentError::NodeHandleError)?;

        // Filter subscriptions only if provided subtopic

        let content_topics = build_content_topics(radio_name, 0, &subtopics);
        let _ = filter_peer_subscriptions(&node_handle, &pubsub_topic, &content_topics)
            .expect("Could not connect and subscribe to the subtopics");

        Ok(GraphcastAgent {
            wallet,
            radio_name,
            pubsub_topic,
            content_topics: Arc::new(Mutex::new(content_topics)),
            node_handle,
            nonces: Arc::new(Mutex::new(HashMap::new())),
            registry_subgraph: registry_subgraph.to_string(),
            network_subgraph: network_subgraph.to_string(),
            graph_node_endpoint: graph_node_endpoint.to_string(),
        })
    }

    /// Get identifiers of Radio content topics
    pub async fn content_identifiers(&self) -> Vec<String> {
        self.content_topics
            .lock()
            .await
            .iter()
            .cloned()
            .map(|topic| topic.content_topic_name.into_owned())
            .collect()
    }

    pub async fn print_subscriptions(&self) {
        info!("pubsub topic: {:#?}", &self.pubsub_topic);
        info!("content topics: {:#?}", &self.content_identifiers().await);
    }

    /// Find the subscribed content topic with an identifier
    /// Error if topic doesn't exist
    pub async fn match_content_topic(
        &self,
        identifier: String,
    ) -> Result<WakuContentTopic, anyhow::Error> {
        debug!(
            "Target content topics: {:#?}\nSubscribed content topics: {:#?}",
            identifier,
            self.content_topics.lock().await,
        );
        match self
            .content_topics
            .lock()
            .await
            .iter()
            .find(|&x| x.content_topic_name == identifier.clone())
        {
            Some(topic) => Ok(topic.clone()),
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
    ) -> Result<(), GraphcastAgentError> {
        let handle_async = move |signal: Signal| {
            let rt = Runtime::new().expect("Could not create Tokio runtime");

            rt.block_on(async {
                let msg = handle_signal(
                    signal,
                    &self.nonces,
                    &self.content_topics.lock().await.clone(),
                    &self.registry_subgraph,
                    &self.network_subgraph,
                    &self.graph_node_endpoint,
                )
                .await;
                let mut radio_handler = radio_handler_mutex.lock().await;
                radio_handler(msg);
            });
        };
        waku_set_event_callback(handle_async);
        Ok(())
    }

    /// For each topic, construct with custom write function and send
    pub async fn send_message<
        T: Message + ethers::types::transaction::eip712::Eip712 + Default + Clone + 'static,
    >(
        &self,
        identifier: String,
        network: NetworkName,
        block_number: u64,
        payload: Option<T>,
    ) -> Result<String, GraphcastAgentError> {
        let content_topic = self
            .match_content_topic(identifier.clone())
            .await
            .map_err(GraphcastAgentError::SendMessageError)?;
        debug!("Selected content topic: {:#?}", content_topic);

        let block_hash = self
            .get_block_hash(network.to_string().clone(), block_number)
            .await?;

        // Check network before sending a message
        network_check(&self.node_handle)
            .map_err(|e| GraphcastAgentError::SendMessageError(e.into()))?;

        GraphcastMessage::build(
            &self.wallet,
            identifier,
            payload,
            network,
            block_number,
            block_hash,
        )
        .await
        .map_err(|e| GraphcastAgentError::SendMessageError(e.into()))?
        .send_to_waku(&self.node_handle, self.pubsub_topic.clone(), content_topic)
        .map_err(|e| GraphcastAgentError::SendMessageError(e.into()))
    }

    pub async fn get_block_hash(
        &self,
        network: String,
        block_number: u64,
    ) -> Result<String, GraphcastAgentError> {
        let hash: String = query_graph_node_network_block_hash(
            self.graph_node_endpoint.to_string(),
            network,
            block_number,
        )
        .await?;
        Ok(hash)
    }

    // TODO: Could register the query function at intialization and call it within this fn
    pub async fn update_content_topics(&self, subtopics: Vec<String>) {
        info!("updating the topics: {:#?}", subtopics);
        // build content topics
        let content_topics = build_content_topics(self.radio_name, 0, &subtopics);
        let old_contents = self.content_topics.lock().await;
        // Check if an update to the content topic is necessary
        if *old_contents != content_topics {
            // subscribe to the new content topics
            let _ =
                filter_peer_subscriptions(&self.node_handle, &self.pubsub_topic, &content_topics)
                    .expect("Could not connect and subscribe to the subtopics");

            //TODO: unsubscribe to the old content topics
            unsubscribe_peer(&self.node_handle, &self.pubsub_topic, &old_contents)
                .expect("Could not connect and subscribe to the subtopics");

            //TODO: need &mut self to update this field, graphcast is usually static so invovles unsafe operation changes
            // optionally content_topics field doesn't necessary need to be with graphcast, or somehow derived when called

            self.content_topics.lock().await.clear();
            self.content_topics.lock().await.extend(content_topics);
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GraphcastAgentError {
    #[error("Query response is empty")]
    EmptyResponseError,
    #[error("Unexpected response format")]
    UnexpectedResponseError,
    #[error(transparent)]
    GraphNodeError(#[from] QueryError),
    #[error("Cannot instantiate Ethereum wallet from given private key.")]
    EthereumWalletError(#[from] WalletError),
    #[error(transparent)]
    UrlParseError(#[from] ParseError),
    #[error("Could not set up node handle. More info: {}", .0)]
    NodeHandleError(WakuHandlingError),
    #[error("Could not send message on the waku networks. More info: {}", .0)]
    SendMessageError(anyhow::Error),
    #[error("Could not parse Waku port")]
    WakuPortError,
    #[error("Unknown error: {0}")]
    Other(anyhow::Error),
}
