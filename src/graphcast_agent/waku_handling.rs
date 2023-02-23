use crate::{
    app_name, cf_nameserver, discovery_url,
    graphcast_agent::message_typing::{self, GraphcastMessage},
    NoncesMap,
};
use colored::*;
use prost::Message;
use std::time::Duration;
use std::{borrow::Cow, env, num::ParseIntError, sync::Arc};
use std::{net::IpAddr, str::FromStr};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use url::ParseError;
use waku::{
    waku_dns_discovery, waku_new, ContentFilter, Encoding, FilterSubscription, Multiaddr,
    ProtocolId, Running, SecretKey, Signal, WakuContentTopic, WakuLogLevel, WakuNodeConfig,
    WakuNodeHandle, WakuPeerData, WakuPubSubTopic,
};

pub const SDK_VERSION: &str = "0";

/// Get pubsub topic based on recommendations from https://rfc.vac.dev/spec/23/
/// With the default namespace of "default"
pub fn pubsub_topic(namespace: Option<&str>) -> WakuPubSubTopic {
    let namespace = namespace.unwrap_or("default");

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
    info!(
        "Subscribe to content topic for filtering: {:#?}",
        subscription
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
    info!("Filter subscription added: {:#?}", filter_subscribe_result);
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
    info!(
        "Unsubscribe content topics on filter protocol: {:#?}",
        subscription
    );
    node_handle
        .filter_unsubscribe(&subscription, Duration::new(6000, 0))
        .map_err(|e| {
            WakuHandlingError::UnsubscribeError(format!(
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
    pubsub_topics: Vec<WakuPubSubTopic>,
) -> Option<WakuNodeConfig> {
    let log_level = match env::var("WAKU_LOG_LEVEL") {
        Ok(level) => match level.to_uppercase().as_str() {
            "DEBUG" => WakuLogLevel::Debug,
            "INFO" => WakuLogLevel::Info,
            "WARN" => WakuLogLevel::Warn,
            "ERROR" => WakuLogLevel::Error,
            "FATAL" => WakuLogLevel::Fatal,
            "PANIC" => WakuLogLevel::Panic,
            _ => WakuLogLevel::Info,
        },
        Err(_) => WakuLogLevel::Warn,
    };

    Some(WakuNodeConfig {
        host: host.and_then(|h| IpAddr::from_str(h).ok()),
        port: Some(port),
        advertise_addr: ad_addr, // Fill this for boot nodes
        node_key: key,
        keep_alive_interval: None,
        relay: Some(true), // Default true - required for filter protocol
        min_peers_to_publish: Some(0), // Default 0
        filter: Some(true), // Default false
        log_level: Some(log_level),
        relay_topics: pubsub_topics,
        discv5: Some(false),
        discv5_bootstrap_nodes: [].to_vec(),
        discv5_udp_port: None,
        store: None,
        database_url: None,
        store_retention_max_messages: None,
        store_retention_max_seconds: None,
    })
}

/// Generate a node instance of 'node_config', connected to the peers using node addresses on specific waku protocol
pub fn connect_nodes(
    node_handle: &WakuNodeHandle<Running>,
    nodes: Vec<Multiaddr>,
) -> Result<(), WakuHandlingError> {
    let all_nodes = match waku_dns_discovery(&discovery_url()?, Some(&cf_nameserver()), None) {
        Ok(a) => {
            info!("{} {:#?}", "Discovered multiaddresses:".green(), a);
            let mut discovered_nodes: Vec<Multiaddr> =
                a.iter().flat_map(|d| d.addresses.iter()).cloned().collect();
            // Should static node be added or just use as fallback?
            discovered_nodes.extend(nodes.into_iter());
            discovered_nodes
        }
        Err(e) => {
            error!(
                "{}{:?}",
                "Could not discover nodes with provided Url, only add static node list: ".yellow(),
                e
            );
            nodes
        }
    };
    // Connect to peers on the filter protocol
    connect_multiaddresses(all_nodes, node_handle, ProtocolId::Filter);

    info!(
        "Initialized node handle\nLocal node peer_id: {:#?}",
        node_handle.peer_id(),
    );

    Ok(())
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
    warn!(
        "Connected to peers: {:#?}\nFailed to connect to: {:#?}",
        connected_peers, unconnected_peers
    )
}

//TODO: Topic discovery DNS and Discv5
//TODO: Filter full node config for boot nodes
/// Set up a waku node given pubsub topics
pub fn setup_node_handle(
    boot_node_addresses: Vec<String>,
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
        Some(x) if x == *"boot" => boot_node_handle(
            boot_node_addresses,
            pubsub_topic,
            host,
            port,
            advertised_addr,
            node_key,
        ),
        _ => {
            info!(
                "{} {:?}",
                "Registering the following pubsub topics: ".cyan(),
                &pubsub_topic
            );

            let node_config = node_config(
                host,
                port,
                advertised_addr,
                node_key,
                vec![pubsub_topic.clone()],
            );

            let static_nodes = boot_node_addresses
                .iter()
                .flat_map(|addr| vec![Multiaddr::from_str(addr).unwrap_or(Multiaddr::empty())])
                .collect::<Vec<_>>();
            info!("Static node list: {:#?}", static_nodes);

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

            connect_nodes(&node_handle, static_nodes)?;
            Ok(node_handle)
        }
    }
}

pub fn boot_node_handle(
    _boot_node_addresses: Vec<String>,
    pubsub_topic: &WakuPubSubTopic,
    host: Option<&str>,
    port: usize,
    advertised_addr: Option<Multiaddr>,
    node_key: Option<SecretKey>,
) -> Result<WakuNodeHandle<Running>, WakuHandlingError> {
    let boot_node_config = node_config(
        host,
        port,
        advertised_addr,
        node_key,
        vec![pubsub_topic.clone()],
    );
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
    info!(
        "Boot node - id: {}, address: {}",
        boot_node_id, boot_node_multiaddress
    );
    Ok(boot_node_handle)
}

/// Parse and validate incoming message
pub async fn handle_signal<
    T: Message + ethers::types::transaction::eip712::Eip712 + Default + Clone + 'static,
>(
    signal: Signal,
    nonces: &Arc<Mutex<NoncesMap>>,
    content_topics: &[WakuContentTopic],
    registry_subgraph: &str,
    network_subgraph: &str,
    graph_node_endpoint: &str,
) -> Result<GraphcastMessage<T>, anyhow::Error> {
    match signal.event() {
        waku::Event::WakuMessage(event) => {
            match <message_typing::GraphcastMessage<T> as Message>::decode(
                event.waku_message().payload(),
            ) {
                Ok(graphcast_message) => {
                    info!(
                        "{}{}",
                        "New message received! Message id: ".bold().cyan(),
                        event.message_id(),
                    );
                    debug!("{}{:?}", "Message: ".cyan(), graphcast_message);

                    // Allow empty subscription when nothing is provided
                    // Remove after waku filter protocol has been tested thoroughly
                    if content_topics.is_empty()
                        | content_topics.iter().any(|content_topic| {
                            content_topic.content_topic_name == graphcast_message.identifier
                        })
                    {
                        match check_message_validity(
                            graphcast_message,
                            nonces,
                            registry_subgraph,
                            network_subgraph,
                            graph_node_endpoint,
                        )
                        .await
                        {
                            Ok(msg) => Ok(msg),
                            Err(err) => Err(anyhow::anyhow!(
                                "{}{:#?}",
                                "Could not handle the message: ".yellow(),
                                err
                            )),
                        }
                    } else {
                        Err(anyhow::anyhow!(format!(
                            "Skipping Waku message with unsubscribed content topic: {:#?}",
                            graphcast_message.identifier
                        )))
                    }
                }
                Err(e) => Err(anyhow::anyhow!(
                    "Waku message not interpretated as a Graphcast message\nError occurred: {:?}",
                    e
                )),
            }
        }

        waku::Event::Unrecognized(data) => Err(anyhow::anyhow!("Unrecognized event!\n {:?}", data)),
        _ => Err(anyhow::anyhow!(
            "Unrecognized signal!\n {:?}",
            serde_json::to_string(&signal)
        )),
    }
}

/// Check validity of the message:
/// Sender check verifies sender's on-chain identity with Graphcast registry
/// Time check verifies that message was from within the acceptable timestamp
/// Block hash check verifies sender's access to valid Ethereum node provider and blocks
/// Nonce check ensures the ordering of the messages and avoids past messages
pub async fn check_message_validity<
    T: Message + ethers::types::transaction::eip712::Eip712 + Default + Clone + 'static,
>(
    graphcast_message: GraphcastMessage<T>,
    nonces: &Arc<Mutex<NoncesMap>>,
    registry_subgraph: &str,
    network_subgraph: &str,
    graph_node_endpoint: &str,
) -> Result<GraphcastMessage<T>, anyhow::Error> {
    graphcast_message
        .valid_sender(registry_subgraph, network_subgraph)
        .await?
        .valid_time()?
        .valid_hash(graph_node_endpoint)
        .await?
        .valid_nonce(nonces)
        .await?;

    info!("{}", "Valid message!".bold().green());
    Ok(graphcast_message.clone())
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
            debug!("Disconnected peer data: {:#?}", &peer);
            node_handle.connect_peer_with_id(peer.peer_id(), None)
        })
        .for_each(|res| {
            if let Err(x) = res {
                warn!("Could not connect to peer: {}", x)
            }
        });
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum WakuHandlingError {
    #[error(transparent)]
    ParseUrlError(#[from] ParseError),
    #[error("Unable to subscribe to pubsub topic. {}", .0)]
    SubscribeError(String),
    #[error("Unable to unsubscribe to pubsub topic. {}", .0)]
    UnsubscribeError(String),
    #[error("Unable to retrieve peers list. {}", .0)]
    RetrievePeersError(String),
    #[error(transparent)]
    ParsePortError(#[from] ParseIntError),
    #[error("Unable to create waku node: {}", .0)]
    CreateNodeError(String),
    #[error("Unable to get peer information: {}", .0)]
    PeerInfoError(String),
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
