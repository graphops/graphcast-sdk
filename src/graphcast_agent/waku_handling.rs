use crate::{
    app_name, cf_nameserver, discovery_url,
    graphcast_agent::message_typing::{self, check_message_validity, GraphcastMessage},
    graphcast_id_address,
    graphql::QueryError,
};
use prost::Message;
use std::{borrow::Cow, env, num::ParseIntError, sync::Arc};
use std::{collections::HashSet, time::Duration};
use std::{net::IpAddr, str::FromStr};
use tokio::sync::Mutex as AsyncMutex;
use tracing::{debug, error, info, trace};
use url::ParseError;
use waku::{
    waku_dns_discovery, waku_new, ContentFilter, Encoding, FilterSubscription, GossipSubParams,
    Multiaddr, ProtocolId, Running, SecretKey, Signal, WakuContentTopic, WakuLogLevel,
    WakuNodeConfig, WakuNodeHandle, WakuPeerData, WakuPubSubTopic,
};

use super::GraphcastAgent;

pub const SDK_VERSION: &str = "0";

/// Get pubsub topic based on recommendations from https://rfc.vac.dev/spec/23/
/// With the default namespace of "testnet"
pub fn pubsub_topic(namespace: Option<&str>) -> WakuPubSubTopic {
    let namespace = namespace.unwrap_or("testnet");

    WakuPubSubTopic {
        topic_name: Cow::from(app_name().to_string() + "-v" + SDK_VERSION + "-" + namespace),
        encoding: Encoding::Proto,
    }
}

// TODO: update to content topics
/// Generate and format content topics based on recommendations from https://rfc.vac.dev/spec/23/
pub fn build_content_topics(
    radio_name: &str,
    radio_version: usize,
    subtopics: &[String],
) -> Vec<WakuContentTopic> {
    (*subtopics
        .iter()
        .map(|subtopic| WakuContentTopic {
            application_name: Cow::from(radio_name.to_string()),
            version: radio_version,
            content_topic_name: Cow::from(subtopic.to_string()),
            encoding: Encoding::Proto,
        })
        .collect::<Vec<WakuContentTopic>>())
    .to_vec()
}

/// Makes a filter subscription from content topics and optionally pubsub topic
/// Strictly use the first of pubsub topics as we assume radios only listen to one network (pubsub topic) at a time
pub fn content_filter_subscription(
    pubsub_topic: &WakuPubSubTopic,
    content_topics: &[WakuContentTopic],
) -> FilterSubscription {
    let filters = (*content_topics
        .iter()
        .map(|topic| ContentFilter::new(topic.clone()))
        .collect::<Vec<ContentFilter>>())
    .to_vec();
    FilterSubscription::new(filters, Some(pubsub_topic.clone()))
}

/// Make filter subscription requests to all peers except for ourselves
/// Return subscription results for each peer
pub fn filter_peer_subscriptions(
    node_handle: &WakuNodeHandle<Running>,
    graphcast_topic: &WakuPubSubTopic,
    content_topics: &[WakuContentTopic],
) -> Result<Vec<String>, WakuHandlingError> {
    let subscription: FilterSubscription =
        content_filter_subscription(graphcast_topic, content_topics);
    debug!(
        peers = tracing::field::debug(&subscription),
        "Subscribe to topics"
    );
    let filter_subscribe_result: Vec<String> = node_handle
        .peers()
        .map_err(WakuHandlingError::RetrievePeersError)?
        .iter()
        .filter(|&peer| {
            // Filter out local peer_id to prevent self dial
            peer.peer_id().as_str()
                != node_handle
                    .peer_id()
                    .expect("Failed to find local node's peer id")
                    .as_str()
        })
        .map(|peer: &WakuPeerData| {
            // subscribe to all other peers
            let filter_res = node_handle.filter_subscribe(
                &subscription,
                peer.peer_id().clone(),
                Duration::new(6000, 0),
            );
            match filter_res {
                Ok(_) => format!(
                    "Success filter subcription request made to peer {}",
                    peer.peer_id(),
                ),
                Err(e) => format!("Filter subcription request failed for peer {e}"),
            }
        })
        .collect();
    info!(
        peers = tracing::field::debug(&filter_subscribe_result),
        "Subscription connections established",
    );
    Ok(filter_subscribe_result)
}

/// Make filter subscription requests to all peers except for ourselves
/// Return subscription results for each peer
pub fn unsubscribe_peer(
    node_handle: &WakuNodeHandle<Running>,
    graphcast_topic: &WakuPubSubTopic,
    content_topics: &[WakuContentTopic],
) -> Result<(), WakuHandlingError> {
    let subscription: FilterSubscription =
        content_filter_subscription(graphcast_topic, content_topics);
    debug!(
        peers = tracing::field::debug(&subscription),
        "Unsubscribe content topics on filter protocol",
    );
    node_handle
        .filter_unsubscribe(&subscription, Duration::new(6000, 0))
        .map_err(|e| {
            WakuHandlingError::ContentTopicsError(format!(
                "Waku node cannot unsubscribe to the topics: {e}"
            ))
        })
}

/// For boot nodes, configure a Waku Relay Node with filter protocol enabled (Waiting on filterFullNode waku-bindings impl). These node route all messages on the subscribed pubsub topic
/// Preferrably also provide advertise_addr and Secp256k1 private key in Hex format (0x123...abc).
///
/// For light nodes, config with relay disabled and filter enabled. These node will route all messages but only pull data for messages matching the subscribed content topics.
fn node_config(
    host: Option<&str>,
    port: usize,
    ad_addr: Option<Multiaddr>,
    key: Option<SecretKey>,
) -> Option<WakuNodeConfig> {
    let log_level = match env::var("WAKU_LOG_LEVEL") {
        Ok(level) => match level.to_uppercase().as_str() {
            "DEBUG" => WakuLogLevel::Debug,
            "INFO" => WakuLogLevel::Info,
            "WARN" => WakuLogLevel::Warn,
            "ERROR" => WakuLogLevel::Error,
            "FATAL" => WakuLogLevel::Fatal,
            "PANIC" => WakuLogLevel::Panic,
            _ => WakuLogLevel::Warn,
        },
        Err(_) => WakuLogLevel::Error,
    };

    let gossipsub_params = GossipSubParams {
        seen_messages_ttl_seconds: Some(1800),
        history_length: Some(100_000),
        ..Default::default()
    };

    Some(WakuNodeConfig {
        host: host.and_then(|h| IpAddr::from_str(h).ok()),
        port: Some(port),
        advertise_addr: ad_addr, // Fill this for boot nodes
        node_key: key,
        keep_alive_interval: None,
        relay: Some(false), // Default true - will receive all msg on relay
        min_peers_to_publish: Some(0), // Default 0
        filter: Some(true), // Default false
        log_level: Some(log_level),
        relay_topics: [].to_vec(),
        discv5: Some(false),
        discv5_bootstrap_nodes: [].to_vec(),
        discv5_udp_port: None,
        store: None,
        database_url: None,
        store_retention_max_messages: None,
        store_retention_max_seconds: None,
        gossipsub_params: Some(gossipsub_params),
    })
}

/// Gather multiaddresses from different sources of Waku nodes to connect as peers
pub fn gather_nodes(
    static_nodes: Vec<Multiaddr>,
    pubsub_topic: &WakuPubSubTopic,
) -> Vec<Multiaddr> {
    debug!(
        nodes = tracing::field::debug(&static_nodes),
        "Static node list"
    );

    let disc_url = discovery_url(pubsub_topic);
    let dns_nodes = match disc_url {
        Ok(url) => match waku_dns_discovery(&url, Some(&cf_nameserver()), None) {
            Ok(a) => {
                debug!(
                    addresses = tracing::field::debug(&a),
                    "Discovered multiaddresses"
                );
                a.iter().flat_map(|d| d.addresses.iter()).cloned().collect()
            }
            Err(e) => {
                error!(
                    error = tracing::field::debug(e),
                    "Could not discover nodes with provided Url, only add static node list: "
                );
                vec![]
            }
        },
        Err(e) => {
            error!(
                error = tracing::field::debug(&e),
                "Could not discover nodes with provided Url, only add static node list"
            );
            vec![]
        }
    };
    //TODO: update to smarter way of combining the nodes when adding Discv5
    let mut nodes = static_nodes;
    nodes.extend(dns_nodes);
    nodes
}

/// Connect to peers from a list of multiaddresses for a specific protocol
fn connect_multiaddresses(
    nodes: Vec<Multiaddr>,
    node_handle: &WakuNodeHandle<Running>,
    protocol_id: ProtocolId,
) {
    let (connected_peers, unconnected_peers): (Vec<_>, Vec<_>) =
        nodes.into_iter().partition(|address| {
            let peer_id = node_handle
                .add_peer(address, protocol_id)
                .unwrap_or_else(|_| String::from("Could not add peer"));
            node_handle.connect_peer_with_id(&peer_id, None).is_ok()
        });
    debug!(
        peers = tracing::field::debug(connected_peers),
        "Connected to peers"
    );
    if !unconnected_peers.is_empty() {
        debug!(
            peers = tracing::field::debug(unconnected_peers),
            "Peers failed to connect"
        );
    }
}

//TODO: Topic discovery DNS and Discv5
/// Set up a waku node given pubsub topics
pub fn setup_node_handle(
    boot_node_addresses: Vec<Multiaddr>,
    pubsub_topic: &WakuPubSubTopic,
    host: Option<&str>,
    port: Option<&str>,
    advertised_addr: Option<Multiaddr>,
    node_key: Option<SecretKey>,
) -> Result<WakuNodeHandle<Running>, WakuHandlingError> {
    let port = port
        .unwrap_or("60000")
        .parse::<usize>()
        .map_err(WakuHandlingError::ParsePortError)?;

    match std::env::args().nth(1) {
        Some(x) if x == *"boot" => {
            boot_node_handle(pubsub_topic, host, port, advertised_addr, node_key)
        }
        _ => {
            let node_config = node_config(host, port, advertised_addr, node_key);

            let node_handle = waku_new(node_config)
                .map_err(|_e| {
                    WakuHandlingError::CreateNodeError(
                        "Could not create Waku light node".to_string(),
                    )
                })?
                .start()
                .map_err(|_e| {
                    WakuHandlingError::CreateNodeError(
                        "Could not start Waku light node".to_string(),
                    )
                })?;
            let nodes = gather_nodes(boot_node_addresses, pubsub_topic);
            // Connect to peers on the filter protocol
            connect_multiaddresses(nodes, &node_handle, ProtocolId::Filter);

            info!(
                id = tracing::field::debug(node_handle.peer_id()),
                "Initialized node handle with local peer_id",
            );

            Ok(node_handle)
        }
    }
}

pub fn boot_node_handle(
    pubsub_topic: &WakuPubSubTopic,
    host: Option<&str>,
    port: usize,
    advertised_addr: Option<Multiaddr>,
    node_key: Option<SecretKey>,
) -> Result<WakuNodeHandle<Running>, WakuHandlingError> {
    let boot_node_config = node_config(host, port, advertised_addr, node_key);
    let boot_node_handle = waku_new(boot_node_config)
        .map_err(|_e| {
            WakuHandlingError::CreateNodeError("Could not create Waku light node".to_string())
        })?
        .start()
        .map_err(|_e| {
            WakuHandlingError::CreateNodeError("Could not start Waku light node".to_string())
        })?;

    // Relay node subscribe pubsub_topic of graphcast
    boot_node_handle
        .relay_subscribe(Some(pubsub_topic.clone()))
        .expect("Could not subscribe to the topic");

    let boot_node_id = boot_node_handle.peer_id().map_err(|_e| {
        WakuHandlingError::PeerInfoError(
            "Could not get node id from local node instance".to_string(),
        )
    })?;
    let boot_node_multiaddress = format!(
        "/ip4/{}/tcp/{}/p2p/{}",
        host.unwrap_or("0.0.0.0"),
        port,
        boot_node_id
    );
    debug!(
        boot_node_id = tracing::field::debug(&boot_node_id),
        boot_node_address = tracing::field::debug(&boot_node_multiaddress),
        "Boot node initialized"
    );
    Ok(boot_node_handle)
}

/// Parse and validate incoming message
pub async fn handle_signal<
    T: Message
        + ethers::types::transaction::eip712::Eip712
        + Default
        + Clone
        + 'static
        + async_graphql::OutputType,
>(
    signal: Signal,
    graphcast_agent: &GraphcastAgent,
) -> Result<GraphcastMessage<T>, WakuHandlingError> {
    // Do not accept messages that were already received or sent by self
    let old_message_ids: &Arc<AsyncMutex<HashSet<String>>> = &graphcast_agent.old_message_ids;
    let mut ids = old_message_ids.lock().await;
    match signal.event() {
        waku::Event::WakuMessage(event) => {
            match <message_typing::GraphcastMessage<T> as Message>::decode(
                event.waku_message().payload(),
            ) {
                Ok(graphcast_message) => {
                    trace!(
                        id = event.message_id(),
                        message = tracing::field::debug(&graphcast_message),
                        "Received message"
                    );
                    if ids.contains(event.message_id()) {
                        return Err(WakuHandlingError::InvalidMessage(
                            "Skip repeated message".to_string(),
                        ));
                    };
                    // Check for content topic and repetitive message id
                    ids.insert(event.message_id().clone());
                    check_message_validity(
                        graphcast_message,
                        &graphcast_agent.nonces,
                        &graphcast_agent.registry_subgraph,
                        &graphcast_agent.network_subgraph,
                        &graphcast_agent.graph_node_endpoint,
                        graphcast_id_address(&graphcast_agent.wallet),
                    )
                    .await
                    .map_err(|e| WakuHandlingError::InvalidMessage(e.to_string()))
                }
                Err(e) => Err(WakuHandlingError::InvalidMessage(format!(
                    "Waku message not interpretated as a Graphcast message\nError occurred: {e:?}"
                ))),
            }
        }

        waku::Event::Unrecognized(data) => Err(WakuHandlingError::InvalidMessage(format!(
            "Unrecognized event!\n {data:?}"
        ))),
        _ => Err(WakuHandlingError::InvalidMessage(format!(
            "Unrecognized signal!\n {:?}",
            serde_json::to_string(&signal)
        ))),
    }
}

// Allow empty subscription when no content topic was created
// TODO: removed after waku filter protocol has been tested thoroughly
pub fn filter_topic_check(content_topics: &[WakuContentTopic], topic: String) -> bool {
    content_topics.is_empty()
        | content_topics
            .iter()
            .any(|content_topic| content_topic.content_topic_name == topic)
}

/// Check for peer connectivity, try to reconnect if there are disconnected peers
pub fn network_check(node_handle: &WakuNodeHandle<Running>) -> Result<(), WakuHandlingError> {
    let binding = node_handle
        .peer_id()
        .expect("Failed to get local node's peer id");
    let local_id = binding.as_str();

    node_handle
        .peers()
        .map_err(WakuHandlingError::RetrievePeersError)?
        .iter()
        // filter for nodes that are not self and disconnected
        .filter(|&peer| (peer.peer_id().as_str() != local_id) & (!peer.connected()))
        .map(|peer: &WakuPeerData| {
            debug!(
                peer = tracing::field::debug(&peer),
                "Disconnected peer data"
            );
            node_handle.connect_peer_with_id(peer.peer_id(), None)
        })
        .for_each(|res| {
            if let Err(e) = res {
                debug!(
                    error = tracing::field::debug(&e),
                    "Could not connect to peer"
                );
            }
        });
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum WakuHandlingError {
    #[error(transparent)]
    ParseUrlError(#[from] ParseError),
    #[error("Subscription error to the content topic. {}", .0)]
    ContentTopicsError(String),
    #[error("Unable to retrieve peers list. {}", .0)]
    RetrievePeersError(String),
    #[error("Unable to publish message to peer: {}", .0)]
    PublishMessage(String),
    #[error("Unable to validate a message from peer: {}", .0)]
    InvalidMessage(String),
    #[error(transparent)]
    ParsePortError(#[from] ParseIntError),
    #[error("Unable to create waku node: {}", .0)]
    CreateNodeError(String),
    #[error("Unable to get peer information: {}", .0)]
    PeerInfoError(String),
    #[error(transparent)]
    QueryResponseError(#[from] QueryError),
    #[error("Unknown error: {0}")]
    Other(anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_topics() {
        let empty_vec = [].to_vec();
        let empty_topic_vec: Vec<Option<WakuPubSubTopic>> = [].to_vec();
        assert_eq!(
            build_content_topics("test", 0, &empty_vec).len(),
            empty_topic_vec.len()
        );
    }

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
