use chrono::Utc;
use client_graph_node::query_graph_node_poi;
use client_network::{
    query_indexer_allocations, query_indexer_stake, query_stake_minimum_requirement,
};
use client_registry::query_registry_indexer;
use colored::*;
use ethers::types::Block;
use ethers::{
    providers::{Http, Middleware, Provider},
    signers::{LocalWallet, Signer},
    types::{RecoveryMessage, Signature, U64},
};
use ethers_contract::EthAbiType;
use ethers_core::types::transaction::eip712::Eip712;
use ethers_derive_eip712::*;
use prost::Message;
use serde::{Deserialize, Serialize};
use std::env;
use std::{str::FromStr, thread::sleep, time::Duration};
use waku::{
    waku_new, waku_set_event_callback, Encoding, Multiaddr, ProtocolId, Running, Signal,
    WakuContentTopic, WakuMessage, WakuNodeHandle, WakuPubSubTopic,
};

mod client_graph_node;
mod client_network;
mod client_registry;
mod query_network;
mod query_proof_of_indexing;
mod query_registry;

#[derive(Debug, Eip712, EthAbiType, Serialize, Deserialize)]
#[eip712(
    name = "RadioPaylod",
    version = "1",
    chain_id = 1,
    verifying_contract = "0x0000000000000000000000000000000000000000"
)]
pub struct RadioPayload {
    ipfs_hash: String,
    npoi: String,
}

impl RadioPayload {
    pub fn new(ipfs_hash: String, npoi: String) -> Self {
        RadioPayload { ipfs_hash, npoi }
    }
}

impl Clone for RadioPayload {
    fn clone(&self) -> Self {
        Self {
            ipfs_hash: self.ipfs_hash.clone(),
            npoi: self.npoi.clone(),
        }
    }
}

unsafe fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
    ::std::slice::from_raw_parts((p as *const T) as *const u8, ::std::mem::size_of::<T>())
}

impl From<RadioPayloadMessage> for RecoveryMessage {
    fn from(m: RadioPayloadMessage) -> RecoveryMessage {
        RecoveryMessage::Data(unsafe { any_as_u8_slice(&m).to_vec() })
    }
}

#[derive(Eip712, EthAbiType, Clone, Message, Serialize, Deserialize)]
#[eip712(
    name = "GraphcastMessage",
    version = "1",
    chain_id = 1,
    verifying_contract = "0x0000000000000000000000000000000000000000"
)]
pub struct GraphcastMessage {
    #[prost(string, tag = "1")]
    subgraph_hash: String,
    #[prost(string, tag = "2")]
    npoi: String,
    #[prost(int64, tag = "3")]
    nonce: i64,
    #[prost(uint64, tag = "4")]
    block_number: u64,
    #[prost(string, tag = "5")]
    block_hash: String,
    #[prost(string, tag = "6")]
    signature: String,
}

impl GraphcastMessage {
    fn new(
        subgraph_hash: String,
        npoi: String,
        nonce: i64,
        block_number: u64,
        block_hash: String,
        signature: String,
    ) -> Self {
        GraphcastMessage {
            subgraph_hash,
            npoi,
            nonce,
            block_number,
            block_hash,
            signature,
        }
    }
}

#[derive(Eip712, EthAbiType, Clone, Message, Serialize, Deserialize)]
#[eip712(
    name = "radio payload",
    version = "1",
    chain_id = 1,
    verifying_contract = "0x0000000000000000000000000000000000000000"
)]
pub struct RadioPayloadMessage {
    #[prost(string, tag = "1")]
    subgraph_hash: String,
    #[prost(string, tag = "2")]
    npoi: String,
}

impl RadioPayloadMessage {
    fn new(subgraph_hash: String, npoi: String) -> Self {
        RadioPayloadMessage {
            subgraph_hash,
            npoi,
        }
    }
}

fn setup_node_handle(graphcast_topics: Vec<WakuPubSubTopic>) -> WakuNodeHandle<Running> {
    println!(
        "{} {:?}",
        "Registering the following topics: ".cyan(),
        graphcast_topics
    );
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
        node_handle.relay_subscribe(Some(topic)).unwrap();
    }

    node_handle
}

#[tokio::main]
async fn main() {
    let test_topic = String::from("QmacQnSgia4iDPWHpeY6aWxesRFdb8o5DKZUx96zZqEWrB");
    let app_name: String = String::from("graphcast");

    let registry_subgraph =
        String::from("https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test");
    let received_from_address = String::from("0x2bc5349585cbbf924026d25a520ffa9e8b51a39b");
    let indexer_address = query_registry_indexer(registry_subgraph, received_from_address).await;
    println!("{} {}", "Indexer address: ".cyan(), indexer_address);

    let network_subgraph = String::from("https://gateway.testnet.thegraph.com/network");
    let indexer_stake =
        query_indexer_stake(network_subgraph.clone(), indexer_address.clone()).await;
    println!("{} {}", "Indexer stake: ".cyan(), indexer_stake);
    let indexer_allocations_temp =
        query_indexer_allocations(network_subgraph.clone(), indexer_address.clone()).await;
    //Temp: test topic for local poi
    let indexer_allocations = [String::from(&test_topic)];
    println!(
        "{}\n{:?}\n{}\n{:?}",
        "Indexer allocations: ".cyan(),
        indexer_allocations_temp,
        "Add test topic:".cyan(),
        indexer_allocations,
    );
    let minimum_req =
        query_stake_minimum_requirement(network_subgraph.clone(), indexer_address.clone()).await;
    println!(
        "{} {}",
        "fetch minimum indexer requirement: ".cyan(),
        minimum_req
    );

    //TODO: refactor topic generation
    let topics: Vec<WakuPubSubTopic> = indexer_allocations
        .into_iter()
        .map(|hash| {
            let borrowed_hash: &str = &hash;
            let topic = app_name.clone() + "-poi-crosschecker-" + borrowed_hash;
            WakuPubSubTopic {
                topic_name: topic,
                encoding: Encoding::Proto,
            }
        })
        .collect::<Vec<WakuPubSubTopic>>();

    //TODO: add Dispute query to the network subgraph endpoint
    //TODO: add setCostModels mutation to the indexer management server url
    // let indexer_management_endpoint = String::from("http://localhost::18000");
    let node_handle = setup_node_handle(topics.clone());

    // HANDLE RECEIVED MESSAGE
    waku_set_event_callback(move |signal: Signal| {
        println!("{}", "New message!".bold().red());
        match signal.event() {
            waku::Event::WakuMessage(event) => {
                match <GraphcastMessage as Message>::decode(event.waku_message().payload()) {
                    Ok(graphcast_message) => {
                        println!(
                            "\n{}\n{} {:?}",
                            "New message received".bold().green(),
                            "Graphcast message:".cyan(),
                            graphcast_message
                        );

                        let signature = Signature::from_str(&graphcast_message.signature).unwrap();
                        let radio_payload = RadioPayloadMessage::new(
                            graphcast_message.subgraph_hash,
                            graphcast_message.npoi,
                        );

                        let encoded_message = radio_payload.encode_eip712().unwrap();
                        let address = signature.recover(encoded_message).unwrap();

                        println!(
                            "{} {}",
                            "Recovered address from incoming message:".cyan(),
                            address
                        );
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
        }
    });

    let provider =
        Provider::<Http>::try_from("https://goerli.infura.io/v3/dc1a550f824a4c6aa428a3376f983145")
            .unwrap();

    // pretend that we have message topic (ipfsHash), block number and block hash, we then query graph node for POI
    let graph_node_endpoint = String::from("http://localhost:8030/graphql");
    let ipfs_hash = &test_topic;
    let mut compare_block = 0;
    let mut curr_block = 0;

    // Main loop
    loop {
        let block_number = U64::as_u64(&provider.get_block_number().await.unwrap()) - 5;

        if curr_block == block_number {
            sleep(Duration::from_secs(2));
            continue;
        }

        println!("{} {}", "🔗 Block number:".cyan(), block_number);
        curr_block = block_number;

        if block_number % 5 == 0 {
            compare_block = block_number + 3;

            let block: Block<_> = provider.get_block(block_number).await.unwrap().unwrap();
            let block_hash = format!("{:#x}", block.hash.unwrap());
            println!("{} {:?}", "Block hash: ".cyan(), block_hash);

            // Just for debugging
            let poi = query_graph_node_poi(
                graph_node_endpoint.clone(),
                ipfs_hash.to_string(),
                block_hash.to_string(),
                block_number.try_into().unwrap(),
            )
            .await
            .unwrap();
            println!("{} {:?}", "POI: ".cyan(), poi);

            let mut res: Vec<WakuPubSubTopic> = Vec::new();

            println!("\n{}", "Constructing message".bold().green());
            //CONSTRUCTING MESSAGE
            for topic in &topics {
                let graph_node_endpoint = String::from("http://localhost:8030/graphql");
                let payload = topic.clone().topic_name;

                let private_key = env::var("PRIVATE_KEY").expect("No private key provided.");
                let wallet = private_key.parse::<LocalWallet>().unwrap();

                let topic_title = topic.clone().topic_name;
                let ipfs_hash: &str = topic_title.split('-').collect::<Vec<_>>()[3];

                // now sending topic name (somehow a hash)
                // query block number and block hash
                // get graph-node queries
                match query_graph_node_poi(
                    graph_node_endpoint,
                    ipfs_hash.to_string(),
                    block_hash.to_string(),
                    block_number.try_into().unwrap(),
                )
                .await
                {
                    Ok(poi) => {
                        // CONSTRUCT MESSAGE
                        let npoi = poi.data.proof_of_indexing;
                        let nonce = Utc::now().timestamp();

                        let radio_payload_message =
                            RadioPayloadMessage::new(ipfs_hash.to_string(), npoi.clone());
                        let msg = radio_payload_message.encode_eip712().unwrap();

                        let sig = wallet
                            .sign_typed_data(&radio_payload_message)
                            .await
                            .unwrap();
                        let address = sig.recover(msg).unwrap();

                        println!(
                            "{}{}\n{}{}\n{}{:?}",
                            "Signature: ".cyan(),
                            sig,
                            "Recovered address: ".cyan(),
                            address,
                            "Radio payload: ".cyan(),
                            radio_payload_message.clone()
                        );

                        let message = GraphcastMessage::new(
                            ipfs_hash.to_string(),
                            npoi,
                            nonce,
                            block_number.try_into().unwrap(),
                            block_hash.to_string(),
                            sig.to_string(),
                        );

                        // Encode the graphcast message in buff and construct waku message
                        let mut buff = Vec::new();
                        Message::encode(&message, &mut buff).expect("Could not encode :(");

                        println!("\n{}", "Sending message".bold().green());

                        let waku_message = WakuMessage::new(
                            buff,
                            WakuContentTopic {
                                application_name: app_name.clone(),
                                version: 1,
                                content_topic_name: format!(
                                    "{}{}{}",
                                    app_name, "-poi-crosschecker-", ipfs_hash
                                ),
                                encoding: Encoding::Proto,
                            },
                            2,
                            Utc::now().timestamp() as usize,
                        );
                        let sent = node_handle.relay_publish_message(
                            &waku_message,
                            Some(topic.clone()),
                            None,
                        );

                        match sent {
                            Ok(message_id) => {
                                println!("{} {}", "Message sent! Id:".cyan(), message_id);
                            }
                            Err(e) => {
                                println!(
                                    "{}\n{:?}",
                                    "An error occurred! More information:".red(),
                                    e
                                );
                            }
                        }

                        println!("{} {:#?}", "Payload:".cyan(), payload.clone());
                        println!("{} {:#?}", "Topic:".cyan(), topic.clone());

                        // Do we need this?
                        res.push(topic.clone());
                    }
                    Err(error) => {
                        println!("No data for topic {}, more context: {}", payload, error);
                    }
                };
            }
        }
        if block_number == compare_block {
            println!("{}", "Compare attestations here".red());
        }
    }
}
