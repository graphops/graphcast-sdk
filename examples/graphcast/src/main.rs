use chrono::Utc;
use ethers::{
    signers::{LocalWallet, Signer},
    types::{RecoveryMessage, Signature},
};
use ethers_contract::EthAbiType;
use ethers_core::types::transaction::eip712::Eip712;
use ethers_derive_eip712::*;
use once_cell::sync::Lazy;
use prost::Message;
use serde::{Deserialize, Serialize};

use client_graph_node::{perform_proof_of_indexing, query_graph_node_poi};
use client_network::{
    perform_indexer_query, query_indexer_allocations, query_indexer_stake,
    query_stake_minimum_requirement,
};
use client_registry::{perform_operator_indexer_query, query_registry_indexer};

use std::env;
use std::{error::Error, str::FromStr, thread::sleep, time::Duration};
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

impl Into<RecoveryMessage> for RadioPayloadMessage {
    fn into(self) -> RecoveryMessage {
        RecoveryMessage::Data(unsafe { any_as_u8_slice(&self).to_vec() })
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
    #[prost(uint32, tag = "3")]
    nonce: u32,
    #[prost(uint32, tag = "4")]
    block_number: u32,
    #[prost(string, tag = "5")]
    block_hash: String,
    #[prost(string, tag = "6")]
    signature: String,
}

impl GraphcastMessage {
    fn new(
        subgraph_hash: String,
        npoi: String,
        nonce: u32,
        block_number: u32,
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
    let mut indexer_allocations_temp =
        query_indexer_allocations(network_subgraph.clone(), indexer_address.clone()).await;
    //Temp: test topic for local poi
    let indexer_allocations = [String::from(
        "QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U",
    )];
    println!(
        "fetch network subgraph indexer_allocations: {:#?}\nAdd test topic: {:#?}",
        indexer_allocations_temp, indexer_allocations,
    );
    let minimum_req =
        query_stake_minimum_requirement(network_subgraph.clone(), indexer_address.clone()).await;
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
            match <GraphcastMessage as Message>::decode(event.waku_message().payload()) {
                Ok(graphcast_message) => {
                    println!("New message received! \n{:?}", graphcast_message);

                    let ipfs_hash = String::from("hash");
                    let npoi = String::from("npoi");
                    let signature = Signature::from_str(&graphcast_message.signature).unwrap();
                    let radio_payload = RadioPayloadMessage::new(ipfs_hash, npoi);

                    let encoded_message = radio_payload.encode_eip712().unwrap();
                    let address = signature.recover(encoded_message).unwrap();

                    println!("{:?}", address);
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
        let ipfs_hash = String::from("hash");
        let npoi = String::from("npoi");
        let nonce = 55;
        let block_number = 4;
        let block_hash = String::from("block hash");

        let private_key = env::var("PRIVATE_KEY").expect("No private key provided.");

        let wallet = private_key.parse::<LocalWallet>().unwrap();

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

        // now sending topic name (somehow a hash)
        // query block number and block hash
        // get graph-node queries
        let poi = match query_graph_node_poi(
            graph_node_endpoint,
            ipfs_hash.to_string(),
            block_hash.clone(),
            block_number,
        )
        .await
        {
            Ok(poi_response) => {
                let poi = poi_response.data.proof_of_indexing;
                // let message = RadioPayloadMessage::new(ipfs_hash.to_string(), poi);
                // let mut buff = Vec::new();
                // Message::encode(&message, &mut buff).expect("Could not encode :(");

                // let waku_message = WakuMessage::new(
                //     buff,
                //     basic_topic.clone(),
                //     2,
                //     Utc::now().timestamp() as usize,
                // );
                // node_handle
                //     .relay_publish_message(&waku_message, topic.clone(), None)
                //     .expect("Could not send message.");

                // CONSTRUCT MESSAGE
                let radio_payload_message =
                    RadioPayloadMessage::new(ipfs_hash.to_string(), npoi.clone());
                let msg = radio_payload_message.encode_eip712().unwrap();
                let sig = wallet
                    .sign_typed_data(&radio_payload_message)
                    .await
                    .unwrap();
                let address = sig.recover(msg).unwrap();
                println!(
            "constructing the message when {:#?} ; \n signature {:#?}\n recover address {:#?}",
            radio_payload_message.clone(),
            sig.to_string(),
            address,
        );
                let message = GraphcastMessage::new(
                    ipfs_hash.to_string(),
                    npoi,
                    nonce,
                    block_number,
                    block_hash,
                    sig.to_string(),
                );

                // Encode the graphcast message in buff and construct waku message
                let mut buff = Vec::new();
                Message::encode(&message, &mut buff).expect("Could not encode :(");

                println!("{:#?}", payload.clone());
                println!("{:#?}", topic.clone());

                res.push(topic.unwrap())
            }
            Err(error) => {
                println!("No data for topic {}", payload);
            }
        };
    }

    println!("welp  {:#?}", poi);

    loop {
        sleep(Duration::new(1, 0));
    }
}
