use chrono::Utc;
use colored::*;
use ethers::types::Block;
use ethers::types::transaction::eip712::Eip712;
use ethers::{
    providers::{Http, Middleware, Provider},
    signers::{LocalWallet, Signer},
    types::{Signature, U64},
};
use prost::Message;
use std::env;
use std::{str::FromStr, thread::sleep, time::Duration};
use waku::{
    waku_set_event_callback, Encoding, Running, Signal,
    WakuPubSubTopic, WakuMessage, WakuContentTopic,
};

use client_graph_node::query_graph_node_poi;
use client_network::{
    query_indexer_stake,
    query_indexer_allocations,
};
use client_registry::query_registry_indexer;
use typing::*;
use data_request::*;
use waku_handling::setup_node_handle;

mod client_graph_node;
mod client_network;
mod client_registry;
mod query_network;
mod query_proof_of_indexing;
mod query_registry;
mod typing;
mod data_request;
mod waku_handling;


#[tokio::main]
async fn main() {
    // Common inputs - refactor to a set-up function?
    let graph_node_endpoint = String::from("http://localhost:8030/graphql");
    let registry_subgraph =
        String::from("https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test");
    let network_subgraph = String::from("https://gateway.testnet.thegraph.com/network");
    let private_key = env::var("PRIVATE_KEY").expect("No private key provided.");
    let wallet = private_key.parse::<LocalWallet>().unwrap();
    let app_name: String = String::from("graphcast");
    let poi_content_topic: WakuContentTopic = WakuContentTopic {
        application_name: app_name.clone(),
        version: 0,
        content_topic_name: String::from("poi-crosschecker"),
        encoding: Encoding::Proto,
    };

    let indexer_address = query_registry_indexer(registry_subgraph, format!("{:?}", wallet.address())).await;
    let indexer_stake =
        query_indexer_stake(network_subgraph.clone(), indexer_address.clone()).await;
    println!("{} {}\n{} {}", "Indexer address: ".cyan(), indexer_address, "Indexer stake: ".cyan(), indexer_stake);


    // For testing
    let test_topic = String::from("QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U");//Temp: test topic for local poi
    // let indexer_allocations: Vec<String> = [String::from(&test_topic)].to_vec();
    println!("testing functionalities: {:#?} ", testing_functionalities().await);
    let indexer_allocations =
        query_indexer_allocations(network_subgraph.clone(), indexer_address.clone()).await;
    
    //TEMP: using None will let message flow through default-waku peer nodes and filtered by graphcast poi-crosschecker as content topic
    // let topics: Vec<Option<WakuPubSubTopic>> = [None].to_vec();
    let topics: Vec<Option<WakuPubSubTopic>> = generate_pubsub_topics(app_name.clone(), indexer_allocations).await;
    let node_handle = setup_node_handle(topics.clone());
    //TODO: add Dispute query to the network subgraph endpoint

    // HANDLE RECEIVED MESSAGE
    waku_set_event_callback(move |signal: Signal| {
        println!("{}", "New message received!".bold().red());
        match signal.event() {
            waku::Event::WakuMessage(event) => {
                match <GraphcastMessage as Message>::decode(event.waku_message().payload()) {
                    Ok(graphcast_message) => {
                        println!(
                            "{} {:?}",
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
                        //TODO: Async typing needed for the block for all the data requests
                        // let indexer_address = query_registry_indexer(registry_subgraph, address.clone()).await;

                        println!(
                            "{} {}\n Operator for indexer {}",
                            "Recovered address from incoming message:".cyan(),
                            address,
                            indexer_address,
                        );
                    }
                    Err(e) => {
                        println!("Waku message not interpretated as a Graphcast message\nError occurred: {:?}", e);
                    }}
            }
            waku::Event::Unrecognized(data) => {
                println!("Unrecognized event!\n {:?}", data);
            }
            _ => {
                println!("signal! {:?}", serde_json::to_string(&signal));
            }
        }
    });

    // This endpoint should be kept private as much as possible
    let provider =
        Provider::<Http>::try_from("_____")
            .unwrap();
    let mut compare_block = 0;
    let mut curr_block = 0;
    
    // Main loop to process Ethereum Blocks
    loop {
        let block_number = U64::as_u64(&provider.get_block_number().await.unwrap()) - 5;

        if curr_block == block_number {
            sleep(Duration::from_secs(6));
            continue;
        }

        println!("{} {}", "🔗 Block number:".cyan(), block_number);
        curr_block = block_number;

        // Send POI message every 1 block for now
        if block_number % 1 == 0 {
            compare_block = block_number + 3;

            let block: Block<_> = provider.get_block(block_number).await.unwrap().unwrap();
            let block_hash = format!("{:#x}", block.hash.unwrap());
            println!("{} {:?}", "Block hash: ".cyan(), block_hash);

            let mut res: Vec<WakuPubSubTopic> = Vec::new();

            println!("\n{}", "Constructing message".bold().green());
            //CONSTRUCTING MESSAGE
            // Might make more sense to loop through indexer_allocation st we don't need to parse ipfs hash
            // for ipfs_hash in indexer_allocations { - but would need topic generation, can refactor this later
            for topic in topics.clone() {
                println!("Waku node id: {:#?} \nsubscribe to topics {:#?}", node_handle.peer_id(), topic.clone());
                let topic_title = topic.clone().unwrap().topic_name;
                let ipfs_hash: &str = topic_title.split('-').collect::<Vec<_>>()[3];

                // now sending topic name (somehow a hash)
                // query block number and block hash
                // get graph-node queries
                match query_graph_node_poi(
                    graph_node_endpoint.clone(),
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
                            poi_content_topic.clone(),
                            2,
                            Utc::now().timestamp() as usize,
                        );
                        let sent = node_handle.relay_publish_message(
                            &waku_message,
                            topic.clone(),
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
                        println!("{} {:#?}", "Topic:".cyan(), topic.clone());

                        // Save result to attest later
                        res.push(topic.clone().unwrap());
                    }
                    Err(error) => {
                        println!("No data for topic {}, more context: {}", ipfs_hash.clone(), error);
                    }
                };
            }
        }
        if block_number == compare_block {
            println!("{}", "Compare attestations here".red());
        }
    }
}
