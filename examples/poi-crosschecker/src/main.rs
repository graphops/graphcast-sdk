#![allow(clippy::await_holding_lock)]
mod attestations;
mod utils;

use crate::attestations::{compare_attestations, save_local_attestation};
use crate::utils::Attestation;
use attestations::attestation_handler;
use colored::*;
use ethers::types::Block;
use ethers::{
    providers::{Http, Middleware, Provider},
    types::U64,
};
use graphcast::gossip_agent::waku_handling::generate_pubsub_topics;
use graphcast::gossip_agent::{GossipAgent, NETWORK_SUBGRAPH};
use graphcast::graphql::client_network::query_network_subgraph;
use graphcast::graphql::query_graph_node_poi;
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use std::{thread::sleep, time::Duration};
use utils::{GOSSIP_AGENT, LOCAL_ATTESTATIONS, REMOTE_ATTESTATIONS};
use waku::WakuPubSubTopic;

#[macro_use]
extern crate partial_application;

#[tokio::main]
async fn main() {
    // Common inputs - refactor to a set-up function?
    let graph_node_endpoint = String::from("http://localhost:8030/graphql");
    let private_key = env::var("PRIVATE_KEY").expect("No private key provided.");
    let eth_node = env::var("ETH_NODE").expect("No ETH URL provided.");

    let _ = REMOTE_ATTESTATIONS.set(Arc::new(Mutex::new(HashMap::new())));
    let _ = LOCAL_ATTESTATIONS.set(Arc::new(Mutex::new(HashMap::new())));

    // Send message every x blocks for which wait y blocks before attestations
    let examination_frequency = 3;
    let wait_block_duration = 2;

    let provider: Provider<Http> = Provider::<Http>::try_from(eth_node.clone()).unwrap();
    let radio_name: &str = "poi-crosschecker";

    let gossip_agent = GossipAgent::new(private_key, eth_node, radio_name)
        .await
        .unwrap();

    _ = GOSSIP_AGENT.set(gossip_agent);
    let indexer_allocations = &GOSSIP_AGENT.get().unwrap().indexer_allocations;

    //Note: using None will let message flow through default-waku peer nodes and filtered by graphcast poi-crosschecker as content topic
    let topics: Vec<Option<WakuPubSubTopic>> =
        generate_pubsub_topics(radio_name, indexer_allocations);

    let radio_handler = Arc::new(Mutex::new(attestation_handler()));
    GOSSIP_AGENT.get().unwrap().register_handler(radio_handler);

    let mut curr_block = 0;
    let mut compare_block: u64 = 0;

    // Main loop for sending messages, can factor out
    // and take radio specific query and parsing for radioPayload
    loop {
        let block_number = U64::as_u64(&provider.get_block_number().await.unwrap()) - 5;

        if curr_block == block_number {
            sleep(Duration::from_secs(5));
            continue;
        }

        println!("{} {}", "🔗 Block number:".cyan(), block_number);
        curr_block = block_number;

        if block_number == compare_block {
            println!("{}", "Comparing attestations".magenta());
            match compare_attestations(
                compare_block - wait_block_duration,
                Arc::clone(REMOTE_ATTESTATIONS.get().unwrap()),
                Arc::clone(LOCAL_ATTESTATIONS.get().unwrap()),
            ) {
                Ok(msg) => {
                    println!("{}", msg.green().bold());
                }
                Err(err) => {
                    println!("{}", err);
                }
            }
        }

        // Send POI message at a fixed frequency
        if block_number % examination_frequency == 0 {
            compare_block = block_number + wait_block_duration;

            let block: Block<_> = provider.get_block(block_number).await.unwrap().unwrap();
            let block_hash = format!("{:#x}", block.hash.unwrap());

            // CONSTRUCTING MESSAGE
            // Radio specific message content query function
            let poi_query = partial!( query_graph_node_poi => graph_node_endpoint.clone(), _, block_hash.to_string(),block_number.try_into().unwrap());

            // Might make more sense to loop through indexer_allocation st we don't need to parse ipfs hash
            // for ipfs_hash in indexer_allocations { - but would need topic generation, can refactor this later
            for topic in topics.clone() {
                let topic_title = topic.clone().unwrap().topic_name;
                let ipfs_hash: &str = topic_title.split('-').collect::<Vec<_>>()[3];

                let my_stake = query_network_subgraph(
                    NETWORK_SUBGRAPH.to_string(),
                    GOSSIP_AGENT.get().unwrap().indexer_address.clone(),
                )
                .await
                .unwrap()
                .indexer_stake();

                if let Ok(Some(content)) = poi_query(ipfs_hash.to_string()).await {
                    let attestation = Attestation {
                        npoi: content.clone(),
                        stake_weight: my_stake,
                        senders: Vec::new(),
                    };

                    save_local_attestation(attestation, ipfs_hash.to_string(), block_number);

                    let res = GOSSIP_AGENT
                        .get()
                        .unwrap()
                        .send_message(topic, block_number, content)
                        .await;

                    match res {
                        Ok(sent) => println!("res!!! {}", sent),
                        Err(e) => println!("eeeer!!! {}", e),
                    };
                }
            }
        }
    }
}
