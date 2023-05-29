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
use self::message_typing::{BuildMessageError, GraphcastMessage};
use self::waku_handling::{
    build_content_topics, filter_peer_subscriptions, handle_signal, network_check, pubsub_topic,
    setup_node_handle, WakuHandlingError,
};
use ethers::signers::WalletError;
use prost::Message;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::Mutex as AsyncMutex;
use tracing::{debug, info, trace};
use url::ParseError;
use waku::{
    waku_set_event_callback, Multiaddr, Running, Signal, WakuContentTopic, WakuNodeHandle,
    WakuPubSubTopic,
};

use crate::callbook::CallBook;
use crate::graphql::client_graph_node::get_indexing_statuses;
use crate::graphql::client_network::query_network_subgraph;
use crate::graphql::client_registry::query_registry_indexer;
use crate::graphql::QueryError;
use crate::networks::NetworkName;
use crate::{build_wallet, graphcast_id_address, GraphcastIdentity, NoncesMap};

pub mod message_typing;
pub mod waku_handling;

/// A constant defining a message expiration limit.
pub const MSG_REPLAY_LIMIT: i64 = 3_600_000;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Validate the input: {0}")]
    ValidateInput(String),
    #[error("Unknown error: {0}")]
    Other(anyhow::Error),
}

#[derive(Clone)]
pub struct GraphcastAgentConfig {
    pub wallet_key: String,
    pub radio_name: &'static str,
    pub registry_subgraph: String,
    pub network_subgraph: String,
    pub graph_node_endpoint: String,
    pub boot_node_addresses: Vec<Multiaddr>,
    pub graphcast_namespace: Option<String>,
    pub subtopics: Vec<String>,
    pub waku_node_key: Option<String>,
    pub waku_host: Option<String>,
    pub waku_port: Option<String>,
    pub waku_addr: Option<String>,
}

impl GraphcastAgentConfig {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        wallet_key: String,
        radio_name: &'static str,
        registry_subgraph: String,
        network_subgraph: String,
        graph_node_endpoint: String,
        boot_node_addresses: Option<Vec<String>>,
        graphcast_namespace: Option<String>,
        subtopics: Option<Vec<String>>,
        waku_node_key: Option<String>,
        waku_host: Option<String>,
        waku_port: Option<String>,
        waku_addr: Option<String>,
    ) -> Result<Self, GraphcastAgentError> {
        let boot_node_addresses = convert_to_multiaddrs(&boot_node_addresses.unwrap_or(vec![]))
            .map_err(|_| GraphcastAgentError::ConvertMultiaddrError)?;

        let config = GraphcastAgentConfig {
            wallet_key,
            radio_name,
            registry_subgraph,
            network_subgraph,
            graph_node_endpoint,
            boot_node_addresses,
            graphcast_namespace,
            subtopics: subtopics.unwrap_or(vec![]),
            waku_node_key,
            waku_host,
            waku_port,
            waku_addr,
        };

        if let Err(e) = config.validate_set_up().await {
            panic!("Could not validate the supplied configurations: {e}")
        }

        Ok(config)
    }

    pub async fn validate_set_up(&self) -> Result<(), ConfigError> {
        let wallet = build_wallet(&self.wallet_key).map_err(|e| {
            ConfigError::ValidateInput(format!(
                "Invalid key to wallet, use private key or mnemonic: {e}"
            ))
        })?;
        let graphcast_id = graphcast_id_address(&wallet);
        // TODO: Implies invalidity for both graphcast id and registry, maybe map_err more specifically
        let indexer = query_registry_indexer(self.registry_subgraph.to_string(), graphcast_id)
            .await
            .map_err(|e| ConfigError::ValidateInput(format!("The registry subgraph did not contain an entry for the Graphcast ID to Indexer: {e}")))?;
        debug!(address = indexer, "Resolved indexer identity");

        let stake = query_network_subgraph(self.network_subgraph.to_string(), indexer)
            .await
            .map_err(|e| {
                ConfigError::ValidateInput(format!(
                    "The network subgraph must contain an entry for the Indexer stake: {e}"
                ))
            })?
            .indexer_stake();
        debug!(stake = stake, "Resolved indexer stake");

        let _ = get_indexing_statuses(self.graph_node_endpoint.to_string())
            .await
            .map_err(|e| {
                ConfigError::ValidateInput(format!(
                    "Graph node endpoint must be able to serve indexing statuses query: {e}"
                ))
            })?;
        Ok(())
    }
}

fn convert_to_multiaddrs(addresses: &[String]) -> Result<Vec<Multiaddr>, ConfigError> {
    let multiaddrs = addresses
        .iter()
        .map(|address| {
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
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(multiaddrs)
}

/// A Graphcast agent representation
pub struct GraphcastAgent {
    /// GraphcastID's wallet, used to sign messages
    pub graphcast_identity: GraphcastIdentity,
    pub node_handle: WakuNodeHandle<Running>,
    /// Graphcast agent waku instance's radio application
    pub radio_name: &'static str,
    /// Graphcast agent waku instance's pubsub topic
    pub pubsub_topic: WakuPubSubTopic,
    /// Graphcast agent waku instance's content topics
    pub content_topics: Arc<AsyncMutex<Vec<WakuContentTopic>>>,
    /// Nonces map for caching sender nonces in each subtopic
    pub nonces: Arc<AsyncMutex<NoncesMap>>,
    /// Callbook that make query requests
    pub callbook: CallBook,
    /// TODO: remove after confirming that gossippub seen_ttl works
    /// A set of message ids sent from the agent
    pub old_message_ids: Arc<AsyncMutex<HashSet<String>>>,
}

impl GraphcastAgent {
    /// Constructs a new Graphcast agent with the provided configuration.
    ///
    /// The `GraphcastAgentConfig` struct contains the following fields used to construct
    /// different components of the Graphcast agent:
    ///
    /// * `graphcast_identity`: Resolves into an Ethereum wallet, Graphcast id, and indexer identity.
    /// * `radio_name`: Used as part of the content topic for the radio application.
    /// * `registry_subgraph`: The subgraph that indexes the registry contracts.
    /// * `network_subgraph`: The subgraph that indexes network metadata.
    /// * `graph_node_endpoint`: The endpoint for the Graph Node.
    /// * `boot_node_addresses`: The addresses of the Waku nodes to connect to.
    /// * `graphcast_namespace`: The namespace to use for the pubsub topic.
    /// * `subtopics`: The subtopics for content topics that the radio subscribes to.
    /// * `waku_node_key`: The private key for the Waku node.
    /// * `waku_host`: The host for the Waku node.
    /// * `waku_port`: The port for the Waku node.
    /// * `waku_addr`: The advertised address to be connected among the Waku peers.
    ///
    /// If the `waku_host`, `waku_port`, or `waku_addr` fields are not provided, the Waku node will
    /// use default values. Similarly, if the `graphcast_namespace` field is not provided, the agent
    /// will default to using the IPFS hashes of the subgraphs that the indexer is allocating to.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let config = GraphcastAgentConfig {
    ///     wallet_key: String::from("1231231231231231231231231231231231231231231231231231231231231230"),
    ///     radio_name: "test_radio",
    ///     registry_subgraph: String::from("https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test"),
    ///     network_subgraph: String::from("https://gateway.testnet.thegraph.com/network"),
    ///     graph_node_endpoint: String::from("https://api.thegraph.com/index-node/graphql"),
    ///     boot_node_addresses: vec![String::from("/ip4/127.0.0.1/tcp/60000/p2p/16Uiu2YAmDEieEqD5dHSG85G8H51FUKByWoZx7byMy9AbMEgjd5iz")],
    ///     graphcast_namespace: Some("test_namespace_in_pubsub_topic"),
    ///     subtopics: vec![String::from("some_subgraph_hash")],
    ///     waku_node_key: Some(String::from("waku_node_key_can_be_same_as_private1231231231231231231231231230")),
    ///     waku_host: Some(String::from("0.0.0.0")),
    ///     waku_port: Some(String::from("60000")),
    ///     waku_addr: Some(String::from("/ip4/321.1.1.2/tcp/60001/p2p/16Uiu2YAmDEieEqD5dHSG85G8H51FUKByWoZx7byMysomeoneelse")),
    /// };
    ///
    /// let agent = GraphcastAgent::new(config).await?;
    /// ```

    pub async fn new(
        GraphcastAgentConfig {
            wallet_key,
            radio_name,
            registry_subgraph,
            network_subgraph,
            graph_node_endpoint,
            boot_node_addresses,
            graphcast_namespace,
            subtopics,
            waku_node_key,
            waku_host,
            waku_port,
            waku_addr,
        }: GraphcastAgentConfig,
    ) -> Result<GraphcastAgent, GraphcastAgentError> {
        let graphcast_identity =
            GraphcastIdentity::new(wallet_key, registry_subgraph.clone()).await?;
        let pubsub_topic: WakuPubSubTopic = pubsub_topic(graphcast_namespace.as_deref());

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
        .map_err(GraphcastAgentError::WakuNodeError)?;

        // Filter subscriptions only if provided subtopic
        let content_topics = build_content_topics(radio_name, 0, &subtopics);
        let _ = filter_peer_subscriptions(&node_handle, &pubsub_topic, &content_topics)
            .expect("Could not connect and subscribe to the subtopics");
        let callbook = CallBook::new(registry_subgraph, network_subgraph, graph_node_endpoint);

        Ok(GraphcastAgent {
            graphcast_identity,
            radio_name,
            pubsub_topic,
            content_topics: Arc::new(AsyncMutex::new(content_topics)),
            node_handle,
            nonces: Arc::new(AsyncMutex::new(HashMap::new())),
            callbook,
            old_message_ids: Arc::new(AsyncMutex::new(HashSet::new())),
        })
    }

    /// Get the number of peers excluding self
    pub fn number_of_peers(&self) -> usize {
        self.node_handle.peer_count().unwrap_or({
            trace!("Could not count the number of peers");
            0
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
        info!(
            pubsub_topic = tracing::field::debug(&self.pubsub_topic),
            content_topic = tracing::field::debug(&self.content_identifiers().await),
            "Subscriptions"
        );
    }

    /// Find the subscribed content topic with an identifier
    /// Error if topic doesn't exist
    pub async fn match_content_topic(
        &self,
        identifier: String,
    ) -> Result<WakuContentTopic, GraphcastAgentError> {
        trace!(topic = identifier, "Target content topic");
        match self
            .content_topics
            .lock()
            .await
            .iter()
            .find(|&x| x.content_topic_name == identifier.clone())
        {
            Some(topic) => Ok(topic.clone()),
            _ => Err(GraphcastAgentError::Other(anyhow::anyhow!(format!(
                "Did not match a content topic with identifier: {identifier}"
            ))))?,
        }
    }

    /// Establish custom handler for incoming Waku messages
    pub fn register_handler<
        F: FnMut(Result<GraphcastMessage<T>, WakuHandlingError>)
            + std::marker::Sync
            + std::marker::Send
            + 'static,
        T: Message
            + ethers::types::transaction::eip712::Eip712
            + Default
            + Clone
            + 'static
            + async_graphql::OutputType,
    >(
        &'static self,
        radio_handler_mutex: Arc<AsyncMutex<F>>,
    ) -> Result<(), GraphcastAgentError> {
        let handle_async = move |signal: Signal| {
            let rt = Runtime::new().expect("Could not create Tokio runtime");
            rt.block_on(async {
                let msg = handle_signal(signal, self).await;
                let mut radio_handler = radio_handler_mutex.lock().await;
                radio_handler(msg);
            });
        };
        waku_set_event_callback(handle_async);
        Ok(())
    }

    /// For each topic, construct with custom write function and send
    #[allow(unused_must_use)]
    pub async fn send_message<
        T: Message
            + ethers::types::transaction::eip712::Eip712
            + Default
            + Clone
            + 'static
            + async_graphql::OutputType,
    >(
        &self,
        identifier: String,
        network: NetworkName,
        block_number: u64,
        payload: Option<T>,
    ) -> Result<String, GraphcastAgentError> {
        let content_topic = self.match_content_topic(identifier.clone()).await?;
        trace!(
            topic = tracing::field::debug(&content_topic),
            "Selected content topic from subscriptions"
        );

        let block_hash = self
            .callbook
            .block_hash(network.to_string().clone(), block_number)
            .await?;

        // Check network before sending a message
        network_check(&self.node_handle).map_err(GraphcastAgentError::WakuNodeError)?;
        let mut ids = self.old_message_ids.lock().await;

        GraphcastMessage::build(
            &self.graphcast_identity.wallet,
            identifier,
            payload,
            network,
            block_number,
            block_hash,
        )
        .await
        .map_err(GraphcastAgentError::MessageError)?
        .send_to_waku(&self.node_handle, self.pubsub_topic.clone(), content_topic)
        .map_err(GraphcastAgentError::WakuNodeError)
        .map(|id| {
            ids.insert(id.clone());
            trace!(id = id, "Sent message");
            id
        })
    }

    // TODO: Could register the query function at intialization and call it within this fn
    pub async fn update_content_topics(&self, subtopics: Vec<String>) {
        // build content topics
        let new_topics = build_content_topics(self.radio_name, 0, &subtopics);
        let mut cur_topics = self.content_topics.lock().await;

        // Check if an update to the content topic is necessary
        if *cur_topics != new_topics {
            debug!(
                new_topics = tracing::field::debug(&new_topics),
                current_topics = tracing::field::debug(&*cur_topics),
                "Updating to new set of content topics"
            );

            // TODO: Uncomment after a release that contains this issue
            // https://github.com/waku-org/go-waku/pull/536/files
            // // Unsubscribe to the old content topics
            // if !cur_topics.is_empty() {
            //     unsubscribe_peer(&self.node_handle, &self.pubsub_topic, &cur_topics)
            //         .expect("Connect and unsubscribe to subtopics");
            // }

            // Subscribe to the new content topics
            filter_peer_subscriptions(&self.node_handle, &self.pubsub_topic, &new_topics)
                .expect("Connect and subscribe to subtopics");
            *cur_topics = new_topics;
        }
        drop(cur_topics);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GraphcastAgentError {
    #[error(transparent)]
    QueryResponseError(#[from] QueryError),
    #[error(transparent)]
    ConfigValidation(#[from] ConfigError),
    #[error("Cannot instantiate Ethereum wallet from given private key.")]
    EthereumWalletError(#[from] WalletError),
    #[error(transparent)]
    UrlParseError(#[from] ParseError),
    #[error("Could not set up node handle. More info: {}", .0)]
    WakuNodeError(WakuHandlingError),
    #[error("Could not build the message: {}", .0)]
    MessageError(BuildMessageError),
    #[error("Could not parse Waku port")]
    WakuPortError,
    #[error("Failed to convert Multiaddr from String")]
    ConvertMultiaddrError,
    #[error("Unknown error: {0}")]
    Other(anyhow::Error),
}
