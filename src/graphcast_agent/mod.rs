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
use self::message_typing::{GraphcastMessage, IdentityValidation, MessageError, RadioPayload};
use self::waku_handling::{
    build_content_topics, filter_peer_subscriptions, handle_signal, pubsub_topic,
    setup_node_handle, WakuHandlingError,
};
use ethers::signers::WalletError;

use serde::{Deserialize, Serialize};

use async_graphql::{self, Result, SimpleObject};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex as SyncMutex};

use tokio::sync::Mutex as AsyncMutex;
use tracing::{debug, error, info, trace, warn};
use url::ParseError;
use waku::{
    waku_set_event_callback, Multiaddr, Running, Signal, WakuContentTopic, WakuMessage,
    WakuNodeHandle, WakuPeerData, WakuPubSubTopic,
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
pub const MSG_REPLAY_LIMIT: u64 = 3_600_000;

// Waku discovery network
pub const WAKU_DISCOVERY_ENR: &str = "enr:-P-4QJI8tS1WTdIQxq_yIrD05oIIW1Xg-tm_qfP0CHfJGnp9dfr6ttQJmHwTNxGEl4Le8Q7YHcmi-kXTtphxFysS11oBgmlkgnY0gmlwhLymh5GKbXVsdGlhZGRyc7hgAC02KG5vZGUtMDEuZG8tYW1zMy53YWt1djIucHJvZC5zdGF0dXNpbS5uZXQGdl8ALzYobm9kZS0wMS5kby1hbXMzLndha3V2Mi5wcm9kLnN0YXR1c2ltLm5ldAYfQN4DiXNlY3AyNTZrMaEDbl1X_zJIw3EAJGtmHMVn4Z2xhpSoUaP5ElsHKCv7hlWDdGNwgnZfg3VkcIIjKIV3YWt1Mg8";

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
    allow_all_content_topics: Option<bool>,
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
        allow_all_content_topics: Option<bool>,
    ) -> Result<Self, GraphcastAgentError> {
        let boot_node_addresses = convert_to_multiaddrs(&boot_node_addresses.unwrap_or_default())
            .map_err(|_| GraphcastAgentError::ConvertMultiaddrError)?;

        let discv5_enrs = discv5_enrs.unwrap_or(vec![WAKU_DISCOVERY_ENR.to_string()]);

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
            subtopics: subtopics.unwrap_or_default(),
            waku_node_key,
            waku_host,
            waku_port,
            waku_addr,
            // Extra handling here to make sure the default behavior is filter protocol disabled
            filter_protocol: Some(filter_protocol.unwrap_or(false)),
            discv5_enrs,
            discv5_port,
            allow_all_content_topics,
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
    pub content_topics: Arc<SyncMutex<Vec<WakuContentTopic>>>,
    /// Nonces map for caching sender nonces in each subtopic
    pub nonces: Arc<AsyncMutex<NoncesMap>>,
    /// Callbook that make query requests
    pub callbook: CallBook,
    /// msg_seen_ttl is for only relay messages they have not seen before, not effective for client nodes
    /// A set of message ids sent from the agent
    pub seen_msg_ids: Arc<SyncMutex<HashSet<String>>>,
    /// Sender identity validation mechanism used by the Graphcast agent
    pub id_validation: IdentityValidation,
    //TODO: Consider deprecating this field as it isn't utilized in network_check anymore
    /// Keeps track of whether Filter protocol is enabled, if false -> we're using Relay protocol
    pub filter_protocol_enabled: bool,
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
            allow_all_content_topics,
        }: GraphcastAgentConfig,
        sender: Sender<WakuMessage>,
    ) -> Result<GraphcastAgent, GraphcastAgentError> {
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

        let seen_msg_ids = Arc::new(SyncMutex::new(HashSet::new()));
        let content_topics = Arc::new(SyncMutex::new(content_topics));
        register_handler(
            sender,
            seen_msg_ids.clone(),
            content_topics.clone(),
            allow_all_content_topics.is_some(),
        )
        .expect("Could not register handler");

        Ok(GraphcastAgent {
            graphcast_identity,
            radio_name,
            pubsub_topic,
            content_topics,
            node_handle,
            nonces: Arc::new(AsyncMutex::new(HashMap::new())),
            callbook,
            seen_msg_ids,
            id_validation,
            filter_protocol_enabled: filter_protocol.is_some(),
        })
    }

    /// Stop a GraphcastAgent instance
    pub fn stop(self) -> Result<(), GraphcastAgentError> {
        trace!("Set an empty event callback");
        waku_set_event_callback(|_| {});
        debug!("Stop Waku gossip node");
        self.node_handle
            .stop()
            .map_err(|e| GraphcastAgentError::WakuNodeError(WakuHandlingError::StopNodeError(e)))?;
        trace!("Drop Arc std sync mutexes");
        drop(self.content_topics);
        drop(self.nonces);
        drop(self.seen_msg_ids);
        Ok(())
    }

    /// Get the number of peers excluding self
    pub fn number_of_peers(&self) -> usize {
        self.node_handle.peer_count().unwrap_or({
            trace!("Could not count the number of peers");
            0
        })
    }

    /// Get Radio content topics in a Vec
    pub fn content_topics(&self) -> Vec<WakuContentTopic> {
        match self.content_topics.lock() {
            Ok(topics) => topics.iter().cloned().collect(),
            Err(e) => {
                debug!(
                    err = e.to_string(),
                    "Graphcast Agent content topics poisoned"
                );
                vec![]
            }
        }
    }

    /// Get identifiers of Radio content topics
    pub fn content_identifiers(&self) -> Vec<String> {
        match self.content_topics.lock() {
            Ok(topics) => topics
                .iter()
                .cloned()
                .map(|topic| topic.content_topic_name.into_owned())
                .collect(),
            Err(e) => {
                debug!(
                    err = e.to_string(),
                    "Graphcast Agent content topics poisoned"
                );
                vec![]
            }
        }
    }

    pub async fn print_subscriptions(&self) {
        info!(
            pubsub_topic = tracing::field::debug(&self.pubsub_topic),
            content_topic = tracing::field::debug(&self.content_identifiers()),
            "Subscriptions"
        );
    }

    /// Find the subscribed content topic with an identifier
    /// Error if topic doesn't exist
    pub fn match_content_topic_identifier(
        &self,
        identifier: &str,
    ) -> Result<WakuContentTopic, GraphcastAgentError> {
        trace!(topic = identifier, "Target content topic");
        match self
            .content_topics()
            .iter()
            .find(|&x| x.content_topic_name == identifier)
        {
            Some(topic) => Ok(topic.clone()),
            _ => Err(GraphcastAgentError::Other(anyhow::anyhow!(format!(
                "Did not match a content topic with identifier: {identifier}"
            ))))?,
        }
    }

    /// Deprecate in favor of GraphcastMessage::<T>::decode()
    pub async fn decode<T>(&self, payload: &[u8]) -> Result<GraphcastMessage<T>, WakuHandlingError>
    where
        T: RadioPayload,
    {
        match <GraphcastMessage<T> as prost::Message>::decode(payload) {
            Ok(graphcast_message) => Ok(graphcast_message),
            Err(e) => Err(WakuHandlingError::InvalidMessage(format!(
                "Waku message not interpretated as a Graphcast message\nError occurred: {e:?}"
            ))),
        }
    }

    /// For each topic, construct with custom write function and send
    #[allow(unused_must_use)]
    pub async fn send_message<T: RadioPayload>(
        &self,
        identifier: &str,
        payload: T,
        nonce: u64,
    ) -> Result<String, GraphcastAgentError> {
        let content_topic = self.match_content_topic_identifier(identifier)?;
        trace!(
            topic = tracing::field::debug(&content_topic),
            "Selected content topic from subscriptions"
        );

        // Check network before sending a message
        self.network_check()
            .map_err(GraphcastAgentError::WakuNodeError)?;
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
            self.seen_msg_ids.lock().unwrap().insert(id.clone());
            trace!(id = id, "Sent message");
            id
        })
    }

    pub fn update_content_topics(&self, subtopics: Vec<String>) {
        // build content topics
        let new_topics = build_content_topics(&self.radio_name, 0, &subtopics);
        let mut cur_topics = self.content_topics.lock().unwrap();
        *cur_topics = new_topics;

        drop(cur_topics);
    }

    /// Get local node peer data
    pub fn local_peer(&self) -> Option<WakuPeerData> {
        let binding = self
            .node_handle
            .peer_id()
            .expect("Failed to get local node's peer id");
        let local_id = binding.as_str();

        let peers = self.node_handle.peers().ok()?;
        peers.into_iter().find(|p| p.peer_id().as_str() == local_id)
    }

    /// Get all peers data aside from the local node
    pub fn peers_data(&self) -> Result<Vec<WakuPeerData>, WakuHandlingError> {
        let binding = self
            .node_handle
            .peer_id()
            .expect("Failed to get local node's peer id");
        let local_id = binding.as_str();

        let peers = self.node_handle.peers();
        trace!(peers = tracing::field::debug(&peers), "Network peers");

        let peers = peers.map_err(WakuHandlingError::RetrievePeersError)?;
        Ok(peers
            .into_iter()
            .filter(|p| p.peer_id().as_str() != local_id)
            .collect())
    }

    /// Check for peer connectivity, try to reconnect if there are disconnected peers
    pub fn network_check(&self) -> Result<(), WakuHandlingError> {
        let peers = self.peers_data()?;

        for peer in peers.iter() {
            if peer
                .protocols()
                .iter()
                .any(|p| p == "/vac/waku/relay/2.0.0")
            {
                if !peer.connected() {
                    if let Err(e) = self.node_handle.connect_peer_with_id(peer.peer_id(), None) {
                        debug!(
                            error = tracing::field::debug(&e),
                            "Could not connect to peer"
                        );
                    }
                }
            } else {
                self.node_handle
                    .disconnect_peer_with_id(peer.peer_id())
                    .unwrap();
            }
        }
        Ok(())
    }

    /// Get connected peers
    pub fn connected_peer_count(&self) -> Result<usize, WakuHandlingError> {
        Ok(self
            .peers_data()?
            .into_iter()
            // filter for nodes that are not self and disconnected
            .filter(|peer| peer.connected())
            .collect::<Vec<WakuPeerData>>()
            .len())
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
    MessageError(MessageError),
    #[error("Could not parse Waku port")]
    WakuPortError,
    #[error("Failed to convert Multiaddr from String")]
    ConvertMultiaddrError,
    #[error("Unknown error: {0}")]
    Other(anyhow::Error),
}

/// Establish handler for incoming Waku messages
pub fn register_handler(
    sender: Sender<WakuMessage>,
    seen_msg_ids: Arc<SyncMutex<HashSet<String>>>,
    content_topics: Arc<SyncMutex<Vec<WakuContentTopic>>>,
    allow_all_content_topics: bool,
) -> Result<(), GraphcastAgentError> {
    let handle_async = move |signal: Signal| {
        let msg = handle_signal(
            signal,
            &seen_msg_ids,
            &content_topics,
            allow_all_content_topics,
        );

        if let Ok(m) = msg {
            match sender.send(m) {
                Ok(_) => trace!("Sent received message to radio operator"),
                Err(e) => error!("Could not send message to channel: {:#?}", e),
            }
        }
    };

    trace!("Registering handler");
    waku_set_event_callback(handle_async);
    Ok(())
}

impl GraphcastAgentError {
    pub fn type_string(&self) -> &'static str {
        match self {
            GraphcastAgentError::QueryResponseError(_) => "QueryResponseError",
            GraphcastAgentError::ConfigValidation(_) => "ConfigValidation",
            GraphcastAgentError::EthereumWalletError(_) => "EthereumWalletError",
            GraphcastAgentError::UrlParseError(_) => "UrlParseError",
            GraphcastAgentError::WakuNodeError(_) => "WakuNodeError",
            GraphcastAgentError::MessageError(_) => "MessageError",
            GraphcastAgentError::WakuPortError => "WakuPortError",
            GraphcastAgentError::ConvertMultiaddrError => "ConvertMultiaddrError",
            GraphcastAgentError::Other(_) => "Other",
        }
    }
}

/// Peer data from known/connected waku nodes
#[derive(Serialize, Deserialize, Clone, Debug, SimpleObject)]
pub struct PeerData {
    /// Waku peer id
    pub peer_id: String,
    /// Supported node protocols
    pub protocols: Vec<String>,
    /// Node available addresses
    pub addresses: Vec<String>,
    /// Already connected flag
    pub connected: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn test_build_message_error_type_string() {
        let error = MessageError::Payload;
        assert_eq!(error.type_string(), "Payload");

        let error = MessageError::Signing;
        assert_eq!(error.type_string(), "Signing");

        // Add more test cases for other variants...
    }

    #[test]
    fn test_graphcast_agent_error_type_string() {
        let error =
            GraphcastAgentError::QueryResponseError(QueryError::Other(anyhow!("test error")));
        assert_eq!(error.type_string(), "QueryResponseError");

        let error =
            GraphcastAgentError::ConfigValidation(ConfigError::Other(anyhow!("test error")));
        assert_eq!(error.type_string(), "ConfigValidation");
    }

    #[test]
    fn test_waku_handling_error_type_string() {
        let error = WakuHandlingError::ContentTopicsError(String::from(
            "Waku node cannot unsubscribe to the topics",
        ));
        assert_eq!(error.type_string(), "ContentTopicsError");

        let error = WakuHandlingError::InvalidMessage(String::from("Invalid message"));
        assert_eq!(error.type_string(), "InvalidMessage");
    }
}
