mod attestations;
mod utils;

use crate::attestations::{
    compare_attestations, save_local_attestation, Attestation,
};
use crate::utils::parse_messages;
use attestations::attestation_handler;
use colored::*;
use ethers::types::Block;
use ethers::{
    providers::{Http, Middleware, Provider},
    types::U64,
};
use graphcast_sdk::gossip_agent::{GossipAgent, NETWORK_SUBGRAPH};
use graphcast_sdk::graphql::client_network::query_network_subgraph;
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use std::{thread::sleep, time::Duration};
use utils::{GOSSIP_AGENT, LOCAL_ATTESTATIONS, MESSAGES, REMOTE_ATTESTATIONS};

/// Radio specific query function to fetch Proof of Indexing for each allocated subgraph
use graphql::query_graph_node_poi;
mod graphql;

#[macro_use]
extern crate partial_application;

#[tokio::main]
async fn main() {
    // Common inputs - refactor to a set-up function?
    let graph_node_endpoint = String::from("http://localhost:8030/graphql");
    let private_key = env::var("PRIVATE_KEY").expect("No private key provided.");
    let eth_node = env::var("ETH_NODE").expect("No ETH URL provided.");

    _ = REMOTE_ATTESTATIONS.set(Arc::new(Mutex::new(HashMap::new())));
    _ = LOCAL_ATTESTATIONS.set(Arc::new(Mutex::new(HashMap::new())));
    _ = MESSAGES.set(Arc::new(Mutex::new(vec![])));

    // Send message every x blocks for which wait y blocks before attestations
    let examination_frequency = 3;
    let wait_block_duration = 2;

    let provider: Provider<Http> = Provider::<Http>::try_from(eth_node.clone()).unwrap();
    let radio_name: &str = "poi-crosschecker";

    let gossip_agent = GossipAgent::new(private_key, eth_node, radio_name)
        .await
        .unwrap();

    _ = GOSSIP_AGENT.set(gossip_agent);

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

            let parsed = parse_messages(Arc::clone(MESSAGES.get().unwrap())).await;

            if let Err(err) = parsed {
                println!(
                    "{}{}",
                    "An error occured while parsing messages: {}".red().bold(),
                    err
                );
            }

            let remote = REMOTE_ATTESTATIONS.get().unwrap();
            let local = LOCAL_ATTESTATIONS.get().unwrap();

            match compare_attestations(
                compare_block - wait_block_duration,
                Arc::clone(remote),
                Arc::clone(local),
            ) {
                Ok(msg) => {
                    println!("{}", msg.green().bold());
                }
                Err(err) => {
                    println!("{}", err);
                }
            }

            remote.lock().unwrap().clear();
            local.lock().unwrap().clear();
        }

        // Send POI message at a fixed frequency
        if block_number % examination_frequency == 0 {
            compare_block = block_number + wait_block_duration;

            let block: Block<_> = provider.get_block(block_number).await.unwrap().unwrap();
            let block_hash = format!("{:#x}", block.hash.unwrap());

            // Radio specific message content query function
            // Function takes in an identifier string and make specific queries regarding the identifier
            // The example here combines a single function provided query endpoint, current block info
            // Then the function gets sent to agent for making identifier independent queries
            let poi_query = partial!( query_graph_node_poi => graph_node_endpoint.clone(), _, block_hash.to_string(),block_number.try_into().unwrap());
            let identifiers = GOSSIP_AGENT.get().unwrap().content_identifiers();

            let my_stake = query_network_subgraph(
                NETWORK_SUBGRAPH.to_string(),
                GOSSIP_AGENT.get().unwrap().indexer_address.clone(),
            )
            .await
            .unwrap()
            .indexer_stake();

            for id in identifiers {
                match poi_query(id.clone()).await {
                    Ok(content) => {
                        let attestation = Attestation {
                            npoi: content.clone(),
                            stake_weight: my_stake.clone(),
                            senders: Vec::new(),
                        };

                        save_local_attestation(attestation, id.clone(), block_number);

                        match GOSSIP_AGENT
                            .get()
                            .unwrap()
                            .send_message(id.clone(), block_number, content)
                            .await
                        {
                            Ok(sent) => println!("{}: {}", "Sent message id:".green(), sent),
                            Err(e) => println!("{}: {}", "Failed to send message".red(), e),
                        };
                    }
                    Err(e) => println!("{}: {}", "Failed to query message".red(), e),
                }
            }
        }
    }
}
