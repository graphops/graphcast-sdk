use std::{net::IpAddr, str::FromStr};

use chrono::Utc;
use colored::*;
use ethers::{types::{Block, Signature, transaction::eip712::Eip712}, providers::{Provider, Http, Middleware}};
use prost::Message;
use waku::{
    waku_new, Multiaddr, ProtocolId, Running, WakuLogLevel, WakuNodeConfig, WakuNodeHandle,
    WakuPubSubTopic, Signal,
};

use crate::{message_typing, client_registry::query_registry_indexer};

const MSG_REPLAY_LIMIT: i64 = 3_600_000;

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

pub fn setup_node_handle(
    graphcast_topics: Vec<Option<WakuPubSubTopic>>,
    boot_node_id: String,
) -> WakuNodeHandle<Running> {
    // run default nodes with peers hosted with pubsub to graphcast topics
    println!(
        "{} {:?}",
        "Registering the following topics: ".cyan(),
        graphcast_topics
    );

    // Would be nice to refactor this construction with waku topic confi
    let nodes = Vec::from([format!("{}{}", "/ip4/0.0.0.0/tcp/6001/p2p/", boot_node_id),
    "/dns4/node-01.ac-cn-hongkong-c.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAkvWiyFsgRhuJEb9JfjYxEkoHLgnUQmr1N5mKWnYjxYRVm".to_string(),
    "/dns4/node-01.do-ams3.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAmPLe7Mzm8TsYUubgCAW1aJoeFScxrLj8ppHFivPo97bUZ".to_string(),
    "/dns4/node-01.gc-us-central1-a.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAmJb2e28qLXxT5kZxVUUoJt72EMzNGXB47Rxx5hw3q4YjS".to_string(),]);

    connect_and_subscribe(nodes, gen_handle(), graphcast_topics)
}

pub fn setup_boot_node_handle(
    graphcast_topics: Vec<Option<WakuPubSubTopic>>,
) -> WakuNodeHandle<Running> {
    let nodes = Vec::from([
        "/dns4/node-01.ac-cn-hongkong-c.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAkvWiyFsgRhuJEb9JfjYxEkoHLgnUQmr1N5mKWnYjxYRVm".to_string(),
        "/dns4/node-01.do-ams3.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAmPLe7Mzm8TsYUubgCAW1aJoeFScxrLj8ppHFivPo97bUZ".to_string(),
        "/dns4/node-01.gc-us-central1-a.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAmJb2e28qLXxT5kZxVUUoJt72EMzNGXB47Rxx5hw3q4YjS".to_string(),
        ]);

    connect_and_subscribe(nodes, gen_handle(), graphcast_topics)
}

//TODO: add Dispute query to the network subgraph endpoint
pub async fn handle_signal(provider:Provider<Http>, signal: Signal) {
    println!("{}", "New message received!".bold().red());
    match signal.event() {
        waku::Event::WakuMessage(event) => {
            match <message_typing::GraphcastMessage as Message>::decode(event.waku_message().payload())
            {
                Ok(graphcast_message) => {
                    println!(
                        "Message id: {}\n{} {:?}",
                        event.message_id(),
                        "Graphcast message:".cyan(),
                        graphcast_message
                    );

                    let signature =
                        Signature::from_str(&graphcast_message.signature).unwrap();
                    let block: Block<_> = provider.get_block(graphcast_message.block_number).await.unwrap().unwrap();
                        
                    let radio_payload = message_typing::RadioPayloadMessage::new(
                        graphcast_message.subgraph_hash.clone(),
                        graphcast_message.npoi.clone(),
                    );

                    let encoded_message = radio_payload.encode_eip712().unwrap();
                    let address = signature.recover(encoded_message).unwrap();
                    let address = format!("{:#x}", address);

                    let registry_subgraph = String::from(
                "https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test",
            );

                    let indexer_address =
                        query_registry_indexer(registry_subgraph, address.to_string())
                            .await;
                    //TODO: handle Error if sender didn't have indexer address registered with 

                    println!(
                        "{} {}\n Operator for indexer {}",
                        "Recovered address from incoming message:".cyan(),
                        address,
                        indexer_address,
                    );

                    // MESSAGE VALIDITY
                    // Assert for timestamp: prevent past message replay
                    let valid_time = |graphcast_message: message_typing::GraphcastMessage| -> bool {
                        //Can store for measuring overall gossip message latency
                        let message_age =
                            Utc::now().timestamp() - graphcast_message.nonce;
                        println!("message age: {}\nmsg limit: {}", message_age, MSG_REPLAY_LIMIT);
                        (0..MSG_REPLAY_LIMIT).contains(&message_age)
                    }; 
                    if !valid_time(graphcast_message.clone()){
                        println!("{}", "Message timestamp outside acceptable range, drop message".yellow());
                        return 
                    }
                    
                    // Assert for block hash (maybe just pass the hash in?)
                    let valid_hash = |graphcast_message: message_typing::GraphcastMessage| -> bool {
                        let block_hash = format!("{:#x}", block.hash.unwrap());
                        println!("generated block hash: {}\ngraphcastMessageHash: {}", block_hash, graphcast_message.block_hash);
                        graphcast_message.block_hash == block_hash
                    };
                    if !valid_hash(graphcast_message.clone()){
                        println!("{}", "Message hash differ from trusted provider response, drop message".yellow());
                        return
                    };

                    println!("{}", "Valid message!".bold().green());
                    // Store message
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
