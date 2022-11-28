use std::{net::IpAddr, str::FromStr};

use colored::*;
use waku::{
    waku_new, Multiaddr, ProtocolId, Running, WakuLogLevel, WakuNodeConfig, WakuNodeHandle,
    WakuPubSubTopic,
};

fn gen_handle() -> WakuNodeHandle<Running> {
    let constants = WakuNodeConfig {
        host: IpAddr::from_str("0.0.0.0").ok(),
        port: Some(6001),
        advertise_addr: None,
        node_key: None,
        keep_alive_interval: None,
        relay: None,                   //default is True
        min_peers_to_publish: Some(1), // Require other peers in the network
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
