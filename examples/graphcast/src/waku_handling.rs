use crate::message_typing::{self, GraphcastMessage};
use colored::*;
use ethers::{
    providers::{Http, Middleware, Provider},
    types::Block,
};
use prost::Message;
use std::io::prelude::*;
use std::{collections::HashMap, fs::File, net::IpAddr, str::FromStr};
use waku::{
    waku_new, Multiaddr, ProtocolId, Running, Signal, WakuLogLevel, WakuNodeConfig, WakuNodeHandle,
    WakuPubSubTopic,
};

fn gen_handle() -> WakuNodeHandle<Running> {
    let constants = WakuNodeConfig {
        host: IpAddr::from_str("0.0.0.0").ok(),
        port: Some(6001),
        advertise_addr: None,
        node_key: None,
        keep_alive_interval: None,
        relay: None,                   //Default True
        min_peers_to_publish: Some(1), //Default 0
        filter: None,
        log_level: Some(WakuLogLevel::Error),
    };

    waku_new(Some(constants)).unwrap().start().unwrap()
}

fn connect_and_subscribe(
    nodes: Vec<String>,
    node_handle: WakuNodeHandle<Running>,
    graphcast_topics: Vec<Option<WakuPubSubTopic>>,
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

    for topic in graphcast_topics {
        node_handle
            .relay_subscribe(topic.clone())
            .expect("Could not subscribe to the topic");
        println!(
            "PubSub peer readiness: {:#?} -> {:#}",
            topic.clone().unwrap().topic_name,
            node_handle.relay_enough_peers(topic).unwrap()
        );
    }

    println!(
        "listening to peers: {:#?}",
        node_handle.listen_addresses().unwrap()
    );

    node_handle
}

pub fn setup_node_handle(graphcast_topics: &[Option<WakuPubSubTopic>]) -> WakuNodeHandle<Running> {
    match std::env::args().nth(1) {
        Some(x) if x == *"boot" => {
            let nodes = Vec::from([
                "/dns4/node-01.ac-cn-hongkong-c.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAkvWiyFsgRhuJEb9JfjYxEkoHLgnUQmr1N5mKWnYjxYRVm".to_string(),
                "/dns4/node-01.do-ams3.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAmPLe7Mzm8TsYUubgCAW1aJoeFScxrLj8ppHFivPo97bUZ".to_string(),
                "/dns4/node-01.gc-us-central1-a.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAmJb2e28qLXxT5kZxVUUoJt72EMzNGXB47Rxx5hw3q4YjS".to_string(),
            ]);
            let boot_node_handle =
                connect_and_subscribe(nodes, gen_handle(), graphcast_topics.to_vec());
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
                "Registering the following topics: ".cyan(),
                graphcast_topics.to_vec()
            );

            // Would be nice to refactor this construction with waku topic confi
            let nodes = Vec::from([format!("{}{}", "/ip4/0.0.0.0/tcp/6001/p2p/", boot_node_id),
            "/dns4/node-01.ac-cn-hongkong-c.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAkvWiyFsgRhuJEb9JfjYxEkoHLgnUQmr1N5mKWnYjxYRVm".to_string(),
            "/dns4/node-01.do-ams3.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAmPLe7Mzm8TsYUubgCAW1aJoeFScxrLj8ppHFivPo97bUZ".to_string(),
            "/dns4/node-01.gc-us-central1-a.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAmJb2e28qLXxT5kZxVUUoJt72EMzNGXB47Rxx5hw3q4YjS".to_string(),]);

            connect_and_subscribe(nodes, gen_handle(), graphcast_topics.to_vec())
        }
    }
}

//TODO: add Dispute query to the network subgraph endpoint
//Curryify if possible - factor out param on provider,
pub async fn handle_signal(
    provider: Provider<Http>,
    local_nonces: &mut HashMap<String, &mut HashMap<String, &mut i64>>,
    signal: Signal,
) {
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
                    //TODO: Add message handler after checking message validity
                    match check_message_validity(graphcast_message, local_nonces, block_hash).await
                    {
                        Ok(msg) => println!("Decoded valid message: {:#?}", msg),
                        Err(err) => {
                            println!("{}{:#?}", "Could not handle the message: ".yellow(), err)
                        }
                    }
                }
                Err(e) => {
                    println!("Waku message not interpretated as a Graphcast message\nError occurred: {:?}", e);
                }
            }
        }
        waku::Event::Unrecognized(data) => {
            println!("Unrecognized event!\n {:?}", data);
        }
        _ => {
            println!("signal! {:?}", serde_json::to_string(&signal));
        }
    }
}

pub async fn check_message_validity(
    graphcast_message: GraphcastMessage,
    _local_nonces: &mut HashMap<String, &mut HashMap<String, &mut i64>>,
    block_hash: String,
) -> Result<GraphcastMessage, anyhow::Error> {
    graphcast_message
        .valid_sender()
        .await?
        .valid_time()?
        .valid_hash(block_hash)?;

    println!("{}", "Valid message!".bold().green());
    // Store message (group POI and sum stake, best to keep track of sender vec) to attest later

    Ok(graphcast_message.clone())
}
