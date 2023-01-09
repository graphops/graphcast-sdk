mod attestations;
mod utils;

use crate::attestations::{compare_attestations, save_local_attestation, Attestation};
use crate::utils::process_messages;
use attestations::attestation_handler;
use colored::*;
use ethers::types::Block;
use ethers::{
    providers::{Http, Middleware, Provider},
    types::U64,
};
use graphcast_sdk::gossip_agent::{GossipAgent, NETWORK_SUBGRAPH};
use graphcast_sdk::graphql::client_network::query_network_subgraph;
use graphcast_sdk::{config_env_var, slack_bot};
use slack_bot::SlackBot;
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use std::{thread::sleep, time::Duration};
use utils::{LocalAttestationsMap, GOSSIP_AGENT, MESSAGES};

/// Radio specific query function to fetch Proof of Indexing for each allocated subgraph
use graphql::query_graph_node_poi;
mod graphql;

#[macro_use]
extern crate partial_application;

#[tokio::main]
async fn main() {
    let graph_node_endpoint = config_env_var("GRAPH_NODE_STATUS_ENDPOINT")
        .expect("Failed to configure graph node endpoint");
    let private_key =
        config_env_var("PRIVATE_KEY").expect("Failed to configure Ethereum wallet private key");
    let eth_node = config_env_var("ETH_NODE").expect("Failed to configure eth node endpoint");
    let slack_token = config_env_var("SLACK_TOKEN").ok();
    let slack_channel = "#graphcast-slackbot".to_string();

    // Option for where to host the waku node instance
    let waku_host = env::var("WAKU_HOST").ok();
    let waku_port = env::var("WAKU_PORT").ok();
    // Send message every x blocks for which wait y blocks before attestations
    let examination_frequency = 3;
    let wait_block_duration = 2;

    let provider: Provider<Http> = Provider::<Http>::try_from(eth_node.clone()).unwrap();
    let radio_name: &str = "poi-crosschecker";

    let gossip_agent = GossipAgent::new(private_key, eth_node, radio_name, waku_host, waku_port)
        .await
        .unwrap();

    _ = GOSSIP_AGENT.set(gossip_agent);
    _ = MESSAGES.set(Arc::new(Mutex::new(vec![])));

    let radio_handler = Arc::new(Mutex::new(attestation_handler()));
    GOSSIP_AGENT.get().unwrap().register_handler(radio_handler);

    let mut curr_block = 0;
    let mut compare_block: u64 = 0;

    let local_attestations: Arc<Mutex<LocalAttestationsMap>> = Arc::new(Mutex::new(HashMap::new()));

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

            let remote_attestations = process_messages(Arc::clone(MESSAGES.get().unwrap())).await;
            match remote_attestations {
                Ok(remote_attestations) => {
                    match compare_attestations(
                        compare_block - wait_block_duration,
                        remote_attestations,
                        Arc::clone(&local_attestations),
                    ) {
                        Ok(msg) => {
                            println!("{}", msg.green().bold());
                        }
                        Err(err) => {
                            println!("Attestation error: {}", err.to_string().yellow().bold());
                            if let Some(ref token) = slack_token {
                                SlackBot::send_webhook(
                                    token.clone(),
                                    &slack_channel,
                                    radio_name.to_string(),
                                    err.to_string(),
                                )
                                .await
                                .expect("Failed to send alert message to slack channel");
                            }
                        }
                    }
                    MESSAGES.get().unwrap().lock().unwrap().clear();
                }
                Err(err) => {
                    println!(
                        "{}{}",
                        "An error occured while parsing messages: {}".red().bold(),
                        err
                    );
                }
            }
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

                        save_local_attestation(
                            &mut local_attestations.lock().unwrap(),
                            attestation,
                            id.clone(),
                            block_number,
                        );

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
