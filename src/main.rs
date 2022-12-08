use colored::*;
use ethers::types::Block;
use ethers::{
    providers::{Http, Middleware, Provider},
    signers::{LocalWallet, Signer},
    types::U64,
};

use std::borrow::Cow;
use std::collections::HashMap;
use std::env;
use std::sync::Mutex;
use std::{thread::sleep, time::Duration};
use tokio::runtime::Runtime;
use waku::{waku_set_event_callback, Encoding, Signal, WakuContentTopic, WakuPubSubTopic};

use crate::constants::NETWORK_SUBGRAPH;
use crate::graphql::client_network::query_network_subgraph;
use crate::graphql::client_registry::query_registry_indexer;
use crate::graphql::query_graph_node_poi;
use crate::waku_handling::handle_signal;
use lazy_static::lazy_static;
use message_typing::*;
use waku_handling::{generate_pubsub_topics, setup_node_handle};

mod constants;
mod graphql;
mod message_typing;
mod waku_handling;

#[macro_use]
extern crate partial_application;

type NoncesMap = HashMap<String, HashMap<String, i64>>;

lazy_static! {
    pub static ref NONCES: Mutex<NoncesMap> = {
        let m = HashMap::new();
        Mutex::new(m)
    };
}

#[tokio::main]
async fn main() {
    // Common inputs - refactor to a set-up function?
    let graph_node_endpoint = String::from("http://localhost:8030/graphql");
    let private_key = env::var("PRIVATE_KEY").expect("No private key provided.");
    let eth_node = env::var("ETH_NODE").expect("No ETH URL provided.");

    // TODO: Remove at some point
    let test_topic = env::var("TEST_TOPIC").expect("No TEST_TOPIC provided.");

    // Send message every x blocks for which wait y blocks before attestations
    let examination_frequency = 2;
    let wait_block_duration = 1;

    let wallet = private_key.parse::<LocalWallet>().unwrap();
    let provider: Provider<Http> = Provider::<Http>::try_from(eth_node.clone()).unwrap();
    let app_name = Cow::from("graphcast");
    let poi_content_topic: WakuContentTopic = WakuContentTopic {
        application_name: app_name.clone(),
        version: 0,
        content_topic_name: Cow::from("poi-crosschecker"),
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
    let indexer_allocations =
        match query_network_subgraph(NETWORK_SUBGRAPH.to_string(), indexer_address.clone()).await {
            Ok(response) => response.indexer_allocations(),
            Err(err) => {
                println!("Failed to initialize with allocations: {}", err);
                [String::from(&test_topic)].to_vec()
            }
        };

    //Note: using None will let message flow through default-waku peer nodes and filtered by graphcast poi-crosschecker as content topic
    let topics: Vec<Option<WakuPubSubTopic>> =
        generate_pubsub_topics(app_name.clone(), &indexer_allocations);

    let handle_async = move |signal: Signal| {
        let rt = Runtime::new().unwrap();
        let provider: Provider<Http> = Provider::<Http>::try_from(eth_node.clone()).unwrap();
        rt.block_on(async {
            handle_signal(provider, signal, &NONCES).await;
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

            //CONSTRUCTING MESSAGE
            let poi_query = partial!( query_graph_node_poi => graph_node_endpoint.clone(), _, block_hash.to_string(),block_number.try_into().unwrap());
            // Might make more sense to loop through indexer_allocation st we don't need to parse ipfs hash
            // for ipfs_hash in indexer_allocations { - but would need topic generation, can refactor this later
            for topic in topics.clone() {
                let topic_title = topic.clone().unwrap().topic_name;
                let ipfs_hash: &str = topic_title.split('-').collect::<Vec<_>>()[3];

                match poi_query(ipfs_hash.to_string()).await {
                    Ok(Some(npoi)) => {
                        // Refactor/hide waku stuff
                        match GraphcastMessage::build(
                            &wallet,
                            ipfs_hash.to_string(),
                            npoi,
                            block_number.try_into().unwrap(),
                            block_hash.to_string(),
                        )
                        .await
                        {
                            Ok(message) => {
                                match message.send_to_waku(
                                    &node_handle,
                                    topic.clone(),
                                    poi_content_topic.clone(),
                                ) {
                                    Ok(message_id) => {
                                        println!("Message sent: {}", message_id);
                                    }
                                    Err(e) => {
                                        println!(
                                            "Failed to send on pub sub topic {:#?}\nError: {}",
                                            topic.clone(),
                                            e
                                        );
                                    }
                                };
                            }
                            Err(e) => {
                                println!("Could not build Graphcast message: {}", e);
                            }
                        }
                    }
                    Ok(None) => {
                        println!(
                            "Skipping send - Empty data returned for topic {}",
                            ipfs_hash
                        );
                    }
                    Err(error) => {
                        println!(
                            "Skipping send - No data for topic {}, more context: {}",
                            ipfs_hash, error
                        );
                    }
                };
            }
        };
        if block_number == compare_block {
            println!("{}", "Compare attestations here".red());
        }
    }
}
