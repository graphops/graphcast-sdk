use crate::{
    app_name, cf_nameserver, discovery_url,
    gossip_agent::{
        message_typing::{self, GraphcastMessage},
        AgentError,
    },
    NoncesMap,
};
use colored::*;
use ethers::providers::{Http, Middleware, Provider};
use prost::Message;
use std::{borrow::Cow, env, sync::Arc};
use std::{error::Error, sync::Mutex, time::Duration};
use std::{net::IpAddr, str::FromStr};
use tracing::{debug, error, info, warn};
use waku::{
    waku_new, ContentFilter, Encoding, FilterSubscription, Multiaddr, ProtocolId, Running,
    SecretKey, Signal, WakuContentTopic, WakuLogLevel, WakuNodeConfig, WakuNodeHandle,
    WakuPeerData, WakuPubSubTopic,
};

/// Get pubsub topic based on recommendations from https://rfc.vac.dev/spec/23/
pub fn pubsub_topic(versioning: &str, chain_id: &str) -> Option<WakuPubSubTopic> {
    let topic = app_name().to_string() + "-v" + versioning + "-" + chain_id;
    Some(WakuPubSubTopic {
        topic_name: Cow::from(topic),
        encoding: Encoding::Proto,
    })
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
pub fn content_filter_subscription(
    pubsub_topic: &Option<WakuPubSubTopic>,
    content_topics: &[WakuContentTopic],
) -> FilterSubscription {
    let filters = (*content_topics
        .iter()
        .map(|topic| ContentFilter::new(topic.clone()))
        .collect::<Vec<ContentFilter>>())
    .to_vec();
    FilterSubscription::new(filters, pubsub_topic.clone())
}

/// Make filter subscription requests to all peers except for ourselves
/// Return subscription results for each peer
pub fn filter_peer_subscriptions(
    node_handle: &WakuNodeHandle<Running>,
    graphcast_topic: &Option<WakuPubSubTopic>,
    content_topics: &[WakuContentTopic],
) -> Result<Vec<String>, Box<dyn Error>> {
    let subscription: FilterSubscription =
        content_filter_subscription(graphcast_topic, content_topics);
    let filter_subscribe_result: Vec<String> = node_handle
        .peers()?
        .iter()
        .filter(|&peer| {
            peer.peer_id().as_str()
                != node_handle
                    .peer_id()
                    .expect("Failed to find local node's peer id")
                    .as_str()
        })
        .map(|peer: &WakuPeerData| {
            let filter_res = node_handle.filter_subscribe(
                &subscription,
                peer.peer_id().clone(),
                Duration::new(6000, 0),
            );
            match filter_res {
                Ok(_) => format!(
                    "Filter subcription request made to: {} - {:#?}",
                    peer.peer_id(),
                    &subscription
                ),
                Err(e) => format!("Failed to filter subscribe with peer: {e}"),
            }
        })
        .collect();
    info!("Filter subscription added: {:#?}", filter_subscribe_result);
    Ok(filter_subscribe_result)
}

/// For boot nodes, configure a Waku Relay Node with filter protocol enabled (Waiting on filterFullNode waku-bindings impl). These node route all messages on the subscribed pubsub topic
/// Preferrably also provide advertise_addr and Secp256k1 private key in Hex format (0x123...abc).
///
/// For light nodes, config with relay disabled and filter enabled. These node will route all messages but only pull data for messages matching the subscribed content topics.
fn node_config(
    host: Option<&str>,
    port: Option<usize>,
    ad_addr: Option<Multiaddr>,
    key: Option<SecretKey>,
    enable_relay: bool,
    enable_filter: bool,
) -> Option<WakuNodeConfig> {
    let log_level = match env::var("LOG_LEVEL") {
        Ok(level) => match level.to_uppercase().as_str() {
            "INFO" => WakuLogLevel::Info,
            "DEBUG" => WakuLogLevel::Debug,
            "WARN" => WakuLogLevel::Warn,
            "ERROR" => WakuLogLevel::Error,
            _ => WakuLogLevel::Info,
        },
        Err(_) => WakuLogLevel::Info,
    };

    Some(WakuNodeConfig {
        host: host.and_then(|h| IpAddr::from_str(h).ok()),
        port,
        advertise_addr: ad_addr, // Fill this for boot nodes
        // node_key: Some(waku_secret_key),
        node_key: key,
        keep_alive_interval: None,
        relay: Some(enable_relay),     // Default true
        min_peers_to_publish: Some(0), // Default 0
        filter: Some(enable_filter),   // Default false
        log_level: Some(log_level),
    })
}

/// Generate a node instance of 'node_config', connected to the peers using node addresses on specific waku protocol
pub fn connect_nodes(node_handle: &WakuNodeHandle<Running>, nodes: Vec<Multiaddr>) {
    let all_nodes = match node_handle.dns_discovery(&discovery_url(), Some(&cf_nameserver()), None)
    {
        Ok(x) => {
            info!("{} {:#?}", "Discovered multiaddresses:".green(), x);
            let mut discovered_nodes = x;
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
    let peer_ids = connect_multiaddresses(all_nodes, node_handle, ProtocolId::Filter);

    info!(
        "Initialized node handle\nLocal node peer_id: {:#?}\nConnected to peers: {:#?}",
        node_handle.peer_id(),
        peer_ids,
    );
}

/// Connect to peers from a list of multiaddresses for a specific protocol
fn connect_multiaddresses(
    nodes: Vec<Multiaddr>,
    node_handle: &WakuNodeHandle<Running>,
    protocol_id: ProtocolId,
) -> Vec<Multiaddr> {
    nodes
        .into_iter()
        .filter(|address| {
            let peer_id = node_handle
                .add_peer(address, protocol_id)
                .unwrap_or_else(|_| String::from("Could not add peer"));
            match node_handle.connect_peer_with_id(peer_id, None) {
                Ok(_) => true,
                Err(e) => {
                    error!("Could not connect to peer: {}", e);
                    false
                }
            }
        })
        .collect::<Vec<Multiaddr>>()
}

//TODO: Topic discovery DNS and Discv5
//TODO: Filter full node config for boot nodes
/// Set up a waku node given pubsub topics
pub fn setup_node_handle(
    boot_node_addresses: Vec<String>,
    graphcast_topic: &Option<WakuPubSubTopic>,
    host: Option<&str>,
    port: Option<usize>,
    advertised_addr: Option<Multiaddr>,
    node_key: Option<SecretKey>,
) -> WakuNodeHandle<Running> {
    match std::env::args().nth(1) {
        Some(x) if x == *"boot" => {
            // Update boot node to declare isFilterFullNode = True
            let boot_node_config = node_config(host, port, advertised_addr, node_key, true, true);
            let boot_node_handle = waku_new(boot_node_config).unwrap().start().unwrap();
            // let peer_ids = connect_multiaddresses(nodes, &node_handle, ProtocolId::Filter);

            // Relay node subscribe pubsub_topic of graphcast
            boot_node_handle
                .relay_subscribe(graphcast_topic.clone())
                .expect("Could not subscribe to the topic");

            let boot_node_id = boot_node_handle
                .peer_id()
                .expect("Could not get node id from local node instance");
            let boot_node_multiaddress = format!(
                "/ip4/{}/tcp/{}/p2p/{}",
                host.unwrap_or("0.0.0.0"),
                port.unwrap_or(60000),
                boot_node_id
            );
            info!(
                "Boot node - id: {}, address: {}",
                boot_node_id, boot_node_multiaddress
            );

            boot_node_handle
        }
        _ => {
            info!(
                "{} {:?}",
                "Registering the following pubsub topics: ".cyan(),
                graphcast_topic
            );

            let node_config = node_config(host, port, advertised_addr, node_key, false, true);

            let static_nodes = boot_node_addresses
                .iter()
                .flat_map(|addr| vec![Multiaddr::from_str(addr).unwrap_or(Multiaddr::empty())])
                .collect::<Vec<_>>();

            let node_handle = waku_new(node_config).unwrap().start().unwrap();
            // let node_handle = connect_nodes(&node_handle, nodes);
            connect_nodes(&node_handle, static_nodes);
            //TODO: remove when filter subscription is enabled
            node_handle
                .relay_subscribe(graphcast_topic.clone())
                .expect("Could not subscribe to the topic");
            node_handle
        }
    }
}

/// Parse and validate incoming message
pub async fn handle_signal<
    T: Message + ethers::types::transaction::eip712::Eip712 + Default + Clone + 'static,
>(
    provider: &Provider<Http>,
    signal: Signal,
    nonces: &Arc<Mutex<NoncesMap>>,
    content_topics: &[WakuContentTopic],
    registry_subgraph: &str,
    network_subgraph: &str,
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

                    if content_topics.is_empty()
                        | content_topics.iter().any(|content_topic| {
                            content_topic.content_topic_name == graphcast_message.identifier
                        })
                    {
                        let block_hash: String = format!(
                            "{:#x}",
                            provider
                                .get_block(graphcast_message.block_number)
                                .await?
                                .ok_or(AgentError::EmptyResponseError)?
                                .hash
                                .ok_or(AgentError::UnexpectedResponseError)?
                        );
                        match check_message_validity(
                            graphcast_message,
                            block_hash,
                            nonces,
                            registry_subgraph,
                            network_subgraph,
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
                        Err(anyhow::anyhow!(
                            "Content topic '{}' not relevant for this Radio, skipping.",
                            graphcast_message.identifier
                        ))
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
/// Sender check verifies sender's on-chain identity with gossip registry
/// Time check verifies that message was from within the acceptable timestamp
/// Block hash check verifies sender's access to valid Ethereum node provider and blocks
/// Nonce check ensures the ordering of the messages and avoids past messages
pub async fn check_message_validity<
    T: Message + ethers::types::transaction::eip712::Eip712 + Default + Clone + 'static,
>(
    graphcast_message: GraphcastMessage<T>,
    block_hash: String,
    nonces: &Arc<Mutex<NoncesMap>>,
    registry_subgraph: &str,
    network_subgraph: &str,
) -> Result<GraphcastMessage<T>, anyhow::Error> {
    graphcast_message
        .valid_sender(registry_subgraph, network_subgraph)
        .await?
        .valid_time()?
        .valid_hash(block_hash)?
        .valid_nonce(nonces)?;

    info!("{}", "Valid message!".bold().green());
    Ok(graphcast_message.clone())
}

/// Check for peer connectivity, try to reconnect if there are disconnected peers
pub fn network_check(node_handle: &WakuNodeHandle<Running>) -> Result<(), Box<dyn Error>> {
    let binding = node_handle
        .peer_id()
        .expect("Failed to get local node's peer id");
    let local_id = binding.as_str();

    node_handle
        .peers()?
        .iter()
        // filter for nodes that are not self and disconnected
        .filter(|&peer| (peer.peer_id().as_str() != local_id) & (!peer.connected()))
        .map(|peer: &WakuPeerData| {
            debug!("Disconnected peer data: {:#?}", &peer);
            node_handle.connect_peer_with_id(peer.peer_id().to_string(), None)
        })
        .for_each(|res| {
            if let Err(x) = res {
                warn!("Could not connect to peer: {}", x)
            }
        });
    Ok(())
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
