use colored::*;
use ethers::types::Block;
use ethers::{
    providers::{Http, Middleware, Provider},
    types::U64,
};
use once_cell::sync::OnceCell;
use std::env;

use graphcast::gossip_agent::waku_handling::generate_pubsub_topics;
use graphcast::gossip_agent::GossipAgent;
use graphcast::graphql::query_graph_node_poi;
use std::{thread::sleep, time::Duration};
use waku::WakuPubSubTopic;

#[macro_use]
extern crate partial_application;

pub static GOSSIP_AGENT: OnceCell<GossipAgent> = OnceCell::new();

#[tokio::main]
async fn main() {
    // Common inputs - refactor to a set-up function?
    let graph_node_endpoint = String::from("http://localhost:8030/graphql");
    let private_key = env::var("PRIVATE_KEY").expect("No private key provided.");
    let eth_node = env::var("ETH_NODE").expect("No ETH URL provided.");

    // Send message every x blocks for which wait y blocks before attestations
    let examination_frequency = 2;
    let wait_block_duration = 1;

    let provider: Provider<Http> = Provider::<Http>::try_from(eth_node.clone()).unwrap();
    let radio_name: &str = "poi-crosschecker";

    let gossip_agent = GossipAgent::new(private_key, eth_node, radio_name)
        .await
        .unwrap();

    let indexer_allocations = &gossip_agent.indexer_allocations;

    //Note: using None will let message flow through default-waku peer nodes and filtered by graphcast poi-crosschecker as content topic
    let topics: Vec<Option<WakuPubSubTopic>> =
        generate_pubsub_topics(radio_name, indexer_allocations);

    if GOSSIP_AGENT.set(gossip_agent).is_ok() {
        GOSSIP_AGENT.get().unwrap().message_handler();
    }

    let mut curr_block = 0;
    let mut compare_block;

    // Main loop for sending messages, can factor out
    // and take radio specific query and parsing for radioPayload
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

            //CONSTRUCTING MESSAGE
            // Radio specific message content query function
            let poi_query = partial!( query_graph_node_poi => graph_node_endpoint.clone(), _, block_hash.to_string(),block_number.try_into().unwrap());

            // Might make more sense to loop through indexer_allocation st we don't need to parse ipfs hash
            // for ipfs_hash in indexer_allocations { - but would need topic generation, can refactor this later
            for topic in topics.clone() {
                let topic_title = topic.clone().unwrap().topic_name;
                let ipfs_hash: &str = topic_title.split('-').collect::<Vec<_>>()[3];

                if let Ok(Some(content)) = poi_query(ipfs_hash.to_string()).await {
                    {
                        let res = GOSSIP_AGENT
                            .get()
                            .unwrap()
                            .gossip_message(topic, block_number, content)
                            .await;

                        match res {
                            Ok(sent) => println!("res!!! {}", sent),
                            Err(e) => println!("eeeer!!! {}", e),
                        };
                    }
                }
            }
            if block_number == compare_block {
                println!("{}", "Compare attestations here".red());
            }
        }
    }
}
