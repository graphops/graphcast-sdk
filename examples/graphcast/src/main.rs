use chrono::Utc;
use colored::*;
use ethers::types::Block;
use ethers::{
    providers::{Http, Middleware, Provider},
    signers::{LocalWallet, Signer},
    types::U64,
};
use num_traits::identities::Zero;
use prost::Message;
use std::collections::HashMap;
use std::env;
use std::{thread::sleep, time::Duration};
use tokio::runtime::Runtime;
use waku::{
    waku_set_event_callback, Encoding, Signal, WakuContentTopic, WakuMessage, WakuPubSubTopic,
};

use crate::client_network::{query_indexer_allocations, query_indexer_stake};
use crate::constants::NETWORK_SUBGRAPH;
use crate::waku_handling::handle_signal;
use client_graph_node::query_graph_node_poi;
use client_network::perform_indexer_query;
use client_registry::query_registry_indexer;
use data_request::*;
use message_typing::*;
use waku_handling::setup_node_handle;

mod client_graph_node;
mod client_network;
mod client_registry;
mod constants;
mod data_request;
mod message_typing;
mod query_network;
mod query_proof_of_indexing;
mod query_registry;
mod waku_handling;

#[macro_use]
extern crate partial_application;

#[tokio::main]
async fn main() {
    // Common inputs - refactor to a set-up function?
    let graph_node_endpoint = String::from("http://localhost:8030/graphql");
    let private_key = env::var("PRIVATE_KEY").expect("No private key provided.");
    let eth_node = env::var("ETH_NODE").expect("No ETH URL provided.");
    // Send message every x blocks for which wait y blocks before attestations
    let examination_frequency = 2;
    let wait_block_duration = 1;

    let mut local_nonces: HashMap<String, &mut HashMap<String, &mut i64>> = HashMap::new();

    let wallet = private_key.parse::<LocalWallet>().unwrap();
    let provider: Provider<Http> = Provider::<Http>::try_from(eth_node.clone()).unwrap();
    let app_name: String = String::from("graphcast");
    let poi_content_topic: WakuContentTopic = WakuContentTopic {
        application_name: app_name.clone(),
        version: 0,
        content_topic_name: String::from("poi-crosschecker"),
        encoding: Encoding::Proto,
    };

    let indexer_address = match query_registry_indexer(
        constants::REGISTRY_SUBGRAPH.to_string(),
        format!("{:?}", wallet.address()),
    )
    .await
    {
        Ok(addr) => addr,
        Err(err) => {
            println!("Could not query indexer from the registry, make sure operator has been registered: {}", err);
            "".to_string()
        }
    };
    let test_topic = String::from("QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U");
    let indexer_allocations =
        match perform_indexer_query(NETWORK_SUBGRAPH.to_string(), indexer_address.clone()).await {
            Ok(response) => {
                match query_indexer_stake(&response).await {
                    Ok(stake) => {
                        println!("Current indexer stake: {:#?}", stake);
                        stake
                    }
                    Err(err) => {
                        println!("Error querying current stake: {:#?}", err);
                        Zero::zero()
                    }
                };

                // Temp: test topic for local poi
                match query_indexer_allocations(response.clone()).await {
                    Ok(allocations) => {
                        println!("Current allocations: {:#?}", allocations);
                        // allocations
                        [String::from(&test_topic)].to_vec()
                    }
                    Err(err) => {
                        let test_topic =
                            String::from("QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U");
                        println!(
                            "Error fetching current allocations : {},\nUse test topic : {}",
                            err, test_topic
                        );
                        [String::from(&test_topic)].to_vec()
                    }
                }
            }
            Err(err) => {
                println!("Failed to initialize with allocations: {}", err);
                [String::from(&test_topic)].to_vec()
            }
        };

    //Note: using None will let message flow through default-waku peer nodes and filtered by graphcast poi-crosschecker as content topic
    let topics: Vec<Option<WakuPubSubTopic>> =
        generate_pubsub_topics(app_name.clone(), indexer_allocations).await;

    let handle_async = move |signal: Signal| {
        let rt = Runtime::new().unwrap();
        let provider: Provider<Http> = Provider::<Http>::try_from(eth_node.clone()).unwrap();
        rt.block_on(async {
            handle_signal(provider, &mut local_nonces, signal).await;
        });
    };

    // Boot node spawning isn't ideal
    //TODO: Boot id shouldn't need to be generated locally for a regular radio, factor to 3LA or the like
    let node_handle = setup_node_handle(&topics);

    // HANDLE RECEIVED MESSAGE
    waku_set_event_callback(handle_async);

    let mut curr_block = 0;
    let mut compare_block = 0;

    // Main loop for sending messages
    loop {
        let block_number = U64::as_u64(&provider.get_block_number().await.unwrap()) - 5;

        if curr_block == block_number {
            sleep(Duration::from_secs(6));
            continue;
        }

        println!("{} {}", "🔗 Block number:".cyan(), block_number);
        curr_block = block_number;

        // Send POI message at a fixed frequency
        if block_number % examination_frequency == 0 {
            compare_block = block_number + wait_block_duration;

            let block: Block<_> = provider.get_block(block_number).await.unwrap().unwrap();
            let block_hash = format!("{:#x}", block.hash.unwrap());
            let mut res: Vec<WakuPubSubTopic> = Vec::new();

            //CONSTRUCTING MESSAGE
            let poi_query = partial!( query_graph_node_poi => graph_node_endpoint.clone(), _, block_hash.to_string(),block_number.try_into().unwrap());
            // Might make more sense to loop through indexer_allocation st we don't need to parse ipfs hash
            // for ipfs_hash in indexer_allocations { - but would need topic generation, can refactor this later
            for topic in topics.clone() {
                let topic_title = topic.clone().unwrap().topic_name;
                let ipfs_hash: &str = topic_title.split('-').collect::<Vec<_>>()[3];

                // Query POI and handle
                match poi_query(ipfs_hash.to_string()).await {
                    Ok(poi) => {
                        println!("\n{}", "Constructing POI message".bold().green());
                        let npoi = poi.data.proof_of_indexing;

                        let sig = wallet
                            .sign_typed_data(&RadioPayloadMessage::new(
                                ipfs_hash.to_string(),
                                npoi.clone(),
                            ))
                            .await
                            .unwrap();

                        let message = GraphcastMessage::new(
                            ipfs_hash.to_string(),
                            npoi,
                            Utc::now().timestamp(),
                            block_number.try_into().unwrap(),
                            block_hash.to_string(),
                            sig.to_string(),
                        );

                        println!(
                            "{}{:#?}\n{}{:#?}",
                            "Encode message: ".cyan(),
                            message.clone(),
                            "and send on pubsub topic: ".cyan(),
                            topic.clone().unwrap().topic_name,
                        );

                        // Encode the graphcast message in buff and construct waku message
                        let mut buff = Vec::new();
                        Message::encode(&message, &mut buff).expect("Could not encode :(");

                        let waku_message = WakuMessage::new(
                            buff,
                            poi_content_topic.clone(),
                            2,
                            Utc::now().timestamp() as usize,
                        );

                        match node_handle.relay_publish_message(&waku_message, topic.clone(), None)
                        {
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
                        // Save result as local attestament to attest later
                        res.push(topic.clone().unwrap());
                    }
                    Err(error) => {
                        println!("No data for topic {}, more context: {}", ipfs_hash, error);
                    }
                };
            }
        };
        if block_number == compare_block {
            println!("{}", "Compare attestations here".red());
        }
    }
}
