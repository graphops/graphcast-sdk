use chrono::Utc;
use once_cell::sync::Lazy;
use prost::Message;
use serde::{Deserialize, Serialize};

use client_graph_node::{perform_proof_of_indexing, query_graph_node_poi};
use client_registry::{perform_operator_indexer_query, query_registry_indexer};
use client_network::{perform_indexer_query,query_stake_minimum_requirement, query_indexer_allocations, query_indexer_stake};

use std::{error::Error, str::FromStr, thread::sleep, time::Duration};
use waku::{
    waku_new, waku_set_event_callback, Encoding, Multiaddr, ProtocolId, Running, Signal,
    WakuContentTopic, WakuMessage, WakuNodeHandle, WakuPubSubTopic,
};

mod client_graph_node;
mod client_network;
mod client_registry;

#[derive(Clone, Message)]
pub struct BasicMessage {
    #[prost(string, tag = "1")]
    payload: String,
}

impl BasicMessage {
    fn new(payload: String) -> Self {
        BasicMessage { payload }
    }
}

fn setup_node_handle(graphcast_topics: Vec<Option<WakuPubSubTopic>>) -> WakuNodeHandle<Running> {
    const NODES: &[&str] = &[
    "/dns4/node-01.ac-cn-hongkong-c.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAkvWiyFsgRhuJEb9JfjYxEkoHLgnUQmr1N5mKWnYjxYRVm",
    "/dns4/node-01.do-ams3.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAmPLe7Mzm8TsYUubgCAW1aJoeFScxrLj8ppHFivPo97bUZ",
    "/dns4/node-01.gc-us-central1-a.wakuv2.test.statusim.net/tcp/30303/p2p/16Uiu2HAmJb2e28qLXxT5kZxVUUoJt72EMzNGXB47Rxx5hw3q4YjS"
];

    let node_handle = waku_new(None).unwrap();
    let node_handle = node_handle.start().unwrap();
    for address in NODES
        .iter()
        .map(|a| Multiaddr::from_str(a).expect("Could not parse address"))
    {
        let peerid = node_handle.add_peer(&address, ProtocolId::Relay).unwrap();
        node_handle.connect_peer_with_id(peerid, None).unwrap();
    }
    // node_handle.relay_subscribe(graphcast_topic)?;

    for topic in graphcast_topics {
        node_handle.relay_subscribe(topic).unwrap();
    }

    node_handle
}

#[tokio::main]
async fn main() {
    let app_name: String = String::from("graphcast");

    let registry_subgraph =
        String::from("https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test");
    let received_from_address = String::from("0x2bc5349585cbbf924026d25a520ffa9e8b51a39b");
    let indexer_address = query_registry_indexer(registry_subgraph, received_from_address).await;
    println!(
        "fetch registry indexer_address, used as local indexer: {:#?}",
        indexer_address
    );

    let network_subgraph = String::from("https://gateway.testnet.thegraph.com/network");
    let indexer_stake =
        query_indexer_stake(network_subgraph.clone(), indexer_address.clone()).await;
    println!("fetch network subgraph indexer_stake: {:#?}", indexer_stake);
    let mut indexer_allocations_temp = query_indexer_allocations(network_subgraph.clone(), indexer_address.clone()).await;
    //Temp: test topic for local poi
    let indexer_allocations = [String::from("QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U")];
    println!(
        "fetch network subgraph indexer_allocations: {:#?}\nAdd test topic: {:#?}",
        indexer_allocations_temp, indexer_allocations, 
    );
    let minimum_req = query_stake_minimum_requirement(network_subgraph.clone(), indexer_address.clone()).await;
    println!("fetch minimum indexer requirement: {:#?}", minimum_req);

    //TODO: refactor topic generation
    let topics: Vec<Option<WakuPubSubTopic>> = indexer_allocations
        .into_iter()
        .map(|hash| {
            let borrowed_hash: &str = &hash;
            let topic = app_name.clone().to_owned() + "-poi-crosschecker-" + borrowed_hash.clone();
            Some(WakuPubSubTopic {
                topic_name: topic,
                encoding: Encoding::Proto,
            })
        })
        .collect::<Vec<Option<WakuPubSubTopic>>>();

    // pretend that we have message topic (ipfsHash), block number and block hash, we then query graph node for POI
    let graph_node_endpoint = String::from("http://localhost:8030/graphql");
    let ipfs_hash = String::from("QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U");
    let block_hash =
        String::from("9462869694a6b2cffef2615cdb3cfb45c03b113c54a4422d071dadb5aa571395");
    let block_number = 7534805;
    let poi = query_graph_node_poi(graph_node_endpoint, ipfs_hash, block_hash, block_number).await;
    println!("fetch graph node poi ---- {:#?}", poi);

    //TODO: add Dispute query to the network subgraph endpoint
    //TODO: add setCostModels mutation to the indexer management server url
    // let indexer_management_endpoint = String::from("http://localhost::18000");

    let node_handle = setup_node_handle(topics.clone());

    // HANDLE RECEIVED MESSAGE
    waku_set_event_callback(move |signal: Signal| match signal.event() {
        waku::Event::WakuMessage(event) => {
            match <BasicMessage as Message>::decode(event.waku_message().payload()) {
                Ok(basic_message) => {
                    println!("New message received! \n{:?}", basic_message);
                }
                Err(e) => {
                    println!("Error occurred!\n {:?}", e);
                }
            }
        }
        waku::Event::Unrecognized(data) => {
            println!("Unrecognized event!\n {:?}", data);
        }
        _ => {
            println!("signal! {:?}", serde_json::to_string(&signal));
        }
    });
    
    let mut res: Vec<WakuPubSubTopic> = Vec::new();

    //SENDING MESSAGE
    for topic in topics {
        let graph_node_endpoint = String::from("http://localhost:8030/graphql");
        let payload = topic.clone().unwrap().topic_name;

        println!("{:#?}", payload.clone());
        println!("{:#?}", topic.clone());

        // now sending topic name (somehow a hash)
        // query block number and block hash
        // get graph-node queries
        let topic_title = topic.clone().unwrap().topic_name;
        let ipfs_hash: &str = topic_title.split('-').collect::<Vec<_>>()[3];

        //TEMP block info, add when eth node connect and event loop for new blocks
        let block_hash =
            String::from("9462869694a6b2cffef2615cdb3cfb45c03b113c54a4422d071dadb5aa571395");
        let block_number = 7534805;

        let basic_topic = WakuContentTopic {
            application_name: String::from("graphcast"),
            version: 0,
            content_topic_name: String::from("poi-crosschecker"),
            encoding: Encoding::Proto,
        };

        let poi =
            match query_graph_node_poi(graph_node_endpoint, ipfs_hash.to_string(), block_hash, block_number)
                .await
            {
                Ok(poi_response) => {
                    let poi = poi_response.data.proof_of_indexing;
                    let message = BasicMessage::new(poi);
                    let mut buff = Vec::new();
                    Message::encode(&message, &mut buff).expect("Could not encode :(");

                    let waku_message = WakuMessage::new(
                        buff,
                        basic_topic.clone(),
                        2,
                        Utc::now().timestamp() as usize,
                    );
                    node_handle
                        .relay_publish_message(&waku_message, topic.clone(), None)
                        .expect("Could not send message.");

                    res.push(topic.unwrap())
                }
                Err(error) => {
                    println!("No data for topic {}", payload);
                }
            };
    }

    println!("welp  {:#?}", res);

    loop {
        sleep(Duration::new(1, 0));
    }
}
