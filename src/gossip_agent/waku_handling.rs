use crate::{
    app_name,
    gossip_agent::message_typing::{self, GraphcastMessage},
    NoncesMap,
};
use colored::*;
use ethers::{
    providers::{Http, Middleware, Provider},
    types::Block,
};
use prost::Message;
use std::sync::Mutex;
use std::{borrow::Cow, io::prelude::*, sync::Arc};
use std::{fs::File, net::IpAddr, str::FromStr};
use waku::{
    waku_new, Encoding, Multiaddr, ProtocolId, Running, Signal, WakuContentTopic, WakuLogLevel,
    WakuNodeConfig, WakuNodeHandle, WakuPubSubTopic,
};

/// Get pubsub topic based on recommendations from https://rfc.vac.dev/spec/23/
pub fn pubsub_topic(versioning: &str) -> Option<WakuPubSubTopic> {
    let topic = app_name().to_string() + "-v" + versioning;
    Some(WakuPubSubTopic {
        topic_name: Cow::from(topic),
        encoding: Encoding::Proto,
    })
}

// TODO: update to content topics
/// Generate and format content topics based on recommendations from https://rfc.vac.dev/spec/23/
pub fn generate_content_topics(
    radio_name: &str,
    radio_version: usize,
    subtopics: &[&str],
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

/// Initialize a Waku Node with configurations.
/// Specify the node to use relay network and require at least one peer before publishing
fn gen_handle() -> WakuNodeHandle<Running> {
    let constants = WakuNodeConfig {
        host: IpAddr::from_str("0.0.0.0").ok(),
        port: Some(6001),
        advertise_addr: None, // Fill this for boot nodes
        node_key: None,
        keep_alive_interval: None,
        relay: None,                   // Default True
        min_peers_to_publish: Some(1), // Default 0
        filter: None,
        log_level: Some(WakuLogLevel::Error),
    };

    waku_new(Some(constants)).unwrap().start().unwrap()
}

/// Connect waku node to peers and subscribe to topics
fn connect_and_subscribe(
    nodes: Vec<String>,
    node_handle: WakuNodeHandle<Running>,
    graphcast_topic: &Option<WakuPubSubTopic>,
) -> WakuNodeHandle<Running> {
    for address in nodes
        .iter()
        .map(|a| Multiaddr::from_str(a).expect("Could not parse address"))
    {
        let peerid = node_handle
            .add_peer(&address, ProtocolId::Relay)
            .unwrap_or_else(|_| String::from("Could not add peer"));
        node_handle.connect_peer_with_id(peerid, None).unwrap();
    }

    node_handle
        .relay_subscribe(graphcast_topic.clone())
        .expect("Could not subscribe to the topic");

    println!(
        "PubSub peer readiness: {:#?} -> {:#}\nlistening to peers: {:#?}",
        graphcast_topic.clone().unwrap().topic_name,
        node_handle
            .relay_enough_peers(graphcast_topic.clone())
            .unwrap(),
        node_handle.listen_addresses().unwrap()
    );

    node_handle
}

//TODO: Topic discovery
/// Set up a waku node given pubsub topics
pub fn setup_node_handle() -> WakuNodeHandle<Running> {
    let graphcast_topic: &Option<WakuPubSubTopic> = &pubsub_topic("1");
    match std::env::args().nth(1) {
        Some(x) if x == *"boot" => {
            let nodes = Vec::from([
                "/dns4/node-01.ac-cn-hongkong-c.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAkvWiyFsgRhuJEb9JfjYxEkoHLgnUQmr1N5mKWnYjxYRVm".to_string(),
                "/dns4/node-01.do-ams3.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAmPLe7Mzm8TsYUubgCAW1aJoeFScxrLj8ppHFivPo97bUZ".to_string(),
                "/dns4/node-01.gc-us-central1-a.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAmJb2e28qLXxT5kZxVUUoJt72EMzNGXB47Rxx5hw3q4YjS".to_string(),
            ]);
            let boot_node_handle = connect_and_subscribe(nodes, gen_handle(), graphcast_topic);
            let boot_node_id = boot_node_handle.peer_id().unwrap();
            println!("Boot node id {}", boot_node_id);

            let mut file = File::create("./boot_node_id.conf").unwrap();
            file.write_all(boot_node_id.as_bytes()).unwrap();
            boot_node_handle
        }
        _ => {
            let mut file = File::open("./boot_node_id.conf").unwrap();
            let mut boot_node_id = String::new();
            file.read_to_string(&mut boot_node_id).unwrap();

            // run default nodes with peers hosted with pubsub to graphcast topics
            println!(
                "{} {:?}",
                "Registering the following pubsub topics: ".cyan(),
                graphcast_topic
            );

            // Would be nice to refactor this construction with waku topic confi
            let nodes = Vec::from([format!("{}{}", "/ip4/0.0.0.0/tcp/6001/p2p/", boot_node_id),
            "/dns4/node-01.ac-cn-hongkong-c.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAkvWiyFsgRhuJEb9JfjYxEkoHLgnUQmr1N5mKWnYjxYRVm".to_string(),
            "/dns4/node-01.do-ams3.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAmPLe7Mzm8TsYUubgCAW1aJoeFScxrLj8ppHFivPo97bUZ".to_string(),
            "/dns4/node-01.gc-us-central1-a.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAmJb2e28qLXxT5kZxVUUoJt72EMzNGXB47Rxx5hw3q4YjS".to_string(),]);

            connect_and_subscribe(nodes, gen_handle(), graphcast_topic)
        }
    }
}

/// Parse and validate incoming message
pub async fn handle_signal(
    provider: &Provider<Http>,
    signal: Signal,
    nonces: &Arc<Mutex<NoncesMap>>,
) -> Result<GraphcastMessage, anyhow::Error> {
    println!("{}", "New message received!".bold().red());
    match signal.event() {
        waku::Event::WakuMessage(event) => {
            match <message_typing::GraphcastMessage as Message>::decode(
                event.waku_message().payload(),
            ) {
                Ok(graphcast_message) => {
                    println!(
                        "Message id: {}\n{} {:?}",
                        event.message_id(),
                        "Graphcast message:".cyan(),
                        graphcast_message
                    );

                    let block: Block<_> = provider
                        .get_block(graphcast_message.block_number)
                        .await
                        .unwrap()
                        .unwrap();
                    let block_hash = format!("{:#x}", block.hash.unwrap());

                    match check_message_validity(graphcast_message, block_hash, nonces).await {
                        Ok(msg) => Ok(msg),
                        Err(err) => Err(anyhow::anyhow!(
                            "{}{:#?}",
                            "Could not handle the message: ".yellow(),
                            err
                        )),
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
pub async fn check_message_validity(
    graphcast_message: GraphcastMessage,
    block_hash: String,
    nonces: &Arc<Mutex<NoncesMap>>,
) -> Result<GraphcastMessage, anyhow::Error> {
    graphcast_message
        .valid_sender()
        .await?
        .valid_time()?
        .valid_hash(block_hash)?
        .valid_nonce(nonces)?;

    println!("{}", "Valid message!".bold().green());
    Ok(graphcast_message.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_topics() {
        let empty_vec = [].to_vec();
        let empty_topic_vec: Vec<Option<WakuPubSubTopic>> = [].to_vec();
        assert_eq!(
            generate_content_topics("test", 0, &empty_vec).len(),
            empty_topic_vec.len()
        );
    }

    #[test]
    fn test_generate_content_topics() {
        let basics = ["Qmyumyum", "Ymqumqum"].to_vec();
        let res = generate_content_topics("some-radio", 0, &basics);
        for i in 0..res.len() {
            assert_eq!(res[i].content_topic_name, basics[i]);
            assert_eq!(res[i].application_name, "some-radio");
        }
    }
}
