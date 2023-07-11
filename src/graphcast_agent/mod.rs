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
use self::message_typing::{BuildMessageError, GraphcastMessage, IdentityValidation};
use self::waku_handling::{
    build_content_topics, filter_peer_subscriptions, handle_signal, network_check, pubsub_topic,
    setup_node_handle, WakuHandlingError,
};
use ethers::signers::WalletError;
use prost::Message;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc, Mutex as SyncMutex};
use tokio::runtime::Runtime;
use tokio::sync::Mutex as AsyncMutex;
use tracing::{debug, error, info, trace, warn};
use url::ParseError;
use waku::{
    waku_set_event_callback, Multiaddr, Running, Signal, WakuContentTopic, WakuMessage,
    WakuNodeHandle, WakuPubSubTopic,
};

use crate::Account;
use crate::{
    build_wallet,
    callbook::CallBook,
    graphcast_agent::waku_handling::relay_subscribe,
    graphql::{client_graph_node::get_indexing_statuses, QueryError},
    wallet_address, GraphcastIdentity, NoncesMap,
};

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
    pub graph_account: String,
    pub radio_name: String,
    pub registry_subgraph: String,
    pub network_subgraph: String,
    pub id_validation: IdentityValidation,
    pub graph_node_endpoint: Option<String>,
    pub boot_node_addresses: Vec<Multiaddr>,
    pub graphcast_namespace: Option<String>,
    pub subtopics: Vec<String>,
    pub waku_node_key: Option<String>,
    pub waku_host: Option<String>,
    pub waku_port: Option<String>,
    pub waku_addr: Option<String>,
    pub filter_protocol: Option<bool>,
    pub discv5_enrs: Vec<String>,
    pub discv5_port: Option<u16>,
}

impl GraphcastAgentConfig {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        wallet_key: String,
        graph_account: String,
        radio_name: String,
        registry_subgraph: String,
        network_subgraph: String,
        id_validation: IdentityValidation,
        graph_node_endpoint: Option<String>,
        boot_node_addresses: Option<Vec<String>>,
        graphcast_namespace: Option<String>,
        subtopics: Option<Vec<String>>,
        waku_node_key: Option<String>,
        waku_host: Option<String>,
        waku_port: Option<String>,
        waku_addr: Option<String>,
        filter_protocol: Option<bool>,
        discv5_enrs: Option<Vec<String>>,
        discv5_port: Option<u16>,
    ) -> Result<Self, GraphcastAgentError> {
        let boot_node_addresses = convert_to_multiaddrs(&boot_node_addresses.unwrap_or(vec![]))
            .map_err(|_| GraphcastAgentError::ConvertMultiaddrError)?;

        let config = GraphcastAgentConfig {
            wallet_key,
            graph_account,
            radio_name,
            registry_subgraph,
            network_subgraph,
            graph_node_endpoint,
            boot_node_addresses,
            graphcast_namespace,
            id_validation,
            subtopics: subtopics.unwrap_or(vec![]),
            waku_node_key,
            waku_host,
            waku_port,
            waku_addr,
            // Extra handling here to make sure the default behavior is filter protocol enabled
            filter_protocol: Some(filter_protocol.unwrap_or(true)),
            discv5_enrs: discv5_enrs.unwrap_or_default(),
            discv5_port,
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
        let graphcast_id = wallet_address(&wallet);
        let account = Account::new(graphcast_id, self.graph_account.clone());

        // Check if messages sent by configured wallet and graph_account will pass the configured id validation
        match account
            .verify(
                &self.network_subgraph,
                &self.registry_subgraph,
                &self.id_validation,
            )
            .await
        {
            Ok(a) => debug!(
                account = tracing::field::debug(&a),
                id_validation = tracing::field::debug(&self.id_validation),
                "Identity used by local sender can be verified"
            ),
            Err(e) => warn!(
                err = tracing::field::debug(&e),
                id_validation = tracing::field::debug(&self.id_validation),
                "Identity used by local sender can not be verified"
            ),
        };
        if let Some(graph_node) = &self.graph_node_endpoint {
            let _ = get_indexing_statuses(graph_node).await.map_err(|e| {
                ConfigError::ValidateInput(format!(
                    "Graph node endpoint must be able to serve indexing statuses query: {e}"
                ))
            })?;
        }
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
    pub radio_name: String,
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
    /// Sender identity validation mechanism used by the Graphcast agent
    pub id_validation: IdentityValidation,
    /// Upon receiving a valid waku signal event of Message type, sender send WakuMessage through mpsc.
    //TODO: currently graphcast agent returns the handle to radio operator, such that radio handler can process WakuMessage however they want. Ideally we should keep WakuMessage within graphcast agent, but for now radio operator is required to deal with decoding wakuMessage to appropriate types to support multi-types
    pub sender: Arc<SyncMutex<Sender<WakuMessage>>>,
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
    /// * `waku_port`: The port for the Waku node.
    /// * `waku_addr`: The advertised address to be connected among the Waku peers.
    /// * `discv5_enrs:`: ENR records to bootstrap peer discovery through Discv5 mechanism
    /// * `discv5_port:`: The port for the Waku node to be discoverable by peers through Discv5.
    /// * `id_validation:`: Sender identity validation mechanism utilized for incoming messages.
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
    ///     discv5_enrs: vec![String::from("enr:-JK4QBcfVXu2YDeSKdjF2xE5EDM5f5E_1Akpkv_yw_byn1adESxDXVLVjapjDvS_ujx6MgWDu9hqO_Az_CbKLJ8azbMBgmlkgnY0gmlwhAVOUWOJc2VjcDI1NmsxoQOUZIqKLk5xkiH0RAFaMGrziGeGxypJ03kOod1-7Pum3oN0Y3CCfJyDdWRwgiMohXdha3UyDQ")],
    ///     discv5_port: Some(String::from("60000")),
    ///     id_validation: Some(IdentityValidation::NoCheck),
    /// };
    ///
    /// let agent = GraphcastAgent::new(config).await?;
    /// ```

    pub async fn new(
        GraphcastAgentConfig {
            wallet_key,
            graph_account,
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
            filter_protocol,
            discv5_enrs,
            discv5_port,
            id_validation,
        }: GraphcastAgentConfig,
    ) -> Result<(GraphcastAgent, Receiver<WakuMessage>), GraphcastAgentError> {
        let graphcast_identity = GraphcastIdentity::new(wallet_key, graph_account.clone()).await?;
        let pubsub_topic: WakuPubSubTopic = pubsub_topic(graphcast_namespace.as_deref());

        let host = waku_host.as_deref();
        let port = waku_port.as_deref();

        let advertised_addr: Option<Multiaddr> =
            waku_addr.and_then(|a| Multiaddr::from_str(&a).ok());
        let node_key = waku_node_key.and_then(|key| waku::SecretKey::from_str(&key).ok());

        let node_handle = setup_node_handle(
            boot_node_addresses,
            &pubsub_topic,
            host,
            port,
            advertised_addr,
            node_key,
            filter_protocol,
            discv5_enrs,
            discv5_port,
        )
        .map_err(GraphcastAgentError::WakuNodeError)?;

        // Filter subscriptions only if provided subtopic
        let content_topics = build_content_topics(&radio_name, 0, &subtopics);
        if filter_protocol.is_some() && !filter_protocol.unwrap() {
            debug!("Filter protocol disabled, subscribe to pubsub topic on the relay protocol");
            relay_subscribe(&node_handle, &pubsub_topic)
                .expect("Could not subscribe to the pubsub topic");
        } else {
            debug!("Filter protocol enabled, filter subscriptions with peers");
            let _ = filter_peer_subscriptions(&node_handle, &pubsub_topic, &content_topics)
                .expect("Could not connect and subscribe to the subtopics");
        }

        let callbook = CallBook::new(registry_subgraph, network_subgraph, graph_node_endpoint);
        let (sender, receiver) = mpsc::channel::<WakuMessage>();

        Ok((
            GraphcastAgent {
                graphcast_identity,
                radio_name,
                pubsub_topic,
                content_topics: Arc::new(AsyncMutex::new(content_topics)),
                node_handle,
                nonces: Arc::new(AsyncMutex::new(HashMap::new())),
                callbook,
                old_message_ids: Arc::new(AsyncMutex::new(HashSet::new())),
                id_validation,
                sender: Arc::new(SyncMutex::new(sender)),
            },
            receiver,
        ))
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
        identifier: &str,
    ) -> Result<WakuContentTopic, GraphcastAgentError> {
        trace!(topic = identifier, "Target content topic");
        match self
            .content_topics
            .lock()
            .await
            .iter()
            .find(|&x| x.content_topic_name == identifier)
        {
            Some(topic) => Ok(topic.clone()),
            _ => Err(GraphcastAgentError::Other(anyhow::anyhow!(format!(
                "Did not match a content topic with identifier: {identifier}"
            ))))?,
        }
    }

    pub async fn decoder<T>(&self, payload: &[u8]) -> Result<GraphcastMessage<T>, WakuHandlingError>
    where
        T: Message
            + ethers::types::transaction::eip712::Eip712
            + Default
            + Clone
            + 'static
            + async_graphql::OutputType,
    {
        let id_validation = self.id_validation.clone();
        let callbook = self.callbook.clone();
        let nonces = self.nonces.clone();
        let local_sender = self.graphcast_identity.graphcast_id.clone();
        match <GraphcastMessage<T> as Message>::decode(payload) {
            Ok(graphcast_message) => {
                trace!("Validating Graphcast fields: {:#?}", graphcast_message);
                // Add radio msg checks
                message_typing::check_message_validity(
                    graphcast_message,
                    &nonces,
                    callbook.clone(),
                    local_sender.clone(),
                    &id_validation,
                )
                .await
                .map_err(|e| WakuHandlingError::InvalidMessage(e.to_string()))
            }
            Err(e) => Err(WakuHandlingError::InvalidMessage(format!(
                "Waku message not interpretated as a Graphcast message\nError occurred: {e:?}"
            ))),
        }
    }

    /// Establish handler for incoming Waku messages
    pub fn register_handler(&'static self) -> Result<(), GraphcastAgentError> {
        let sender = self.sender.clone();
        let old_message_ids: &Arc<AsyncMutex<HashSet<String>>> = &self.old_message_ids;
        let handle_async = move |signal: Signal| {
            let rt = Runtime::new().expect("Could not create Tokio runtime");
            rt.block_on(async {
                let msg = handle_signal(signal, old_message_ids).await;

                if let Ok(m) = msg {
                    match sender.clone().lock().unwrap().send(m) {
                        Ok(_) => trace!("Sent received message to radio operator"),
                        Err(e) => error!("Could not send message to channel: {:#?}", e),
                    }
                }
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
        identifier: &str,
        payload: T,
        nonce: i64,
    ) -> Result<String, GraphcastAgentError> {
        let content_topic = self.match_content_topic(identifier).await?;
        trace!(
            topic = tracing::field::debug(&content_topic),
            "Selected content topic from subscriptions"
        );

        // Check network before sending a message
        network_check(&self.node_handle).map_err(GraphcastAgentError::WakuNodeError)?;
        let mut ids = self.old_message_ids.lock().await;
        trace!(
            address = &wallet_address(&self.graphcast_identity.wallet),
            "local sender id"
        );
        GraphcastMessage::build(
            &self.graphcast_identity.wallet,
            identifier.to_string(),
            self.graphcast_identity.graph_account.clone(),
            nonce,
            payload,
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
        let new_topics = build_content_topics(&self.radio_name, 0, &subtopics);
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
