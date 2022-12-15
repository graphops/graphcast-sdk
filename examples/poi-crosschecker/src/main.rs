#![allow(clippy::await_holding_lock)]
use colored::*;
use ethers::types::Block;
use ethers::{
    providers::{Http, Middleware, Provider},
    types::U64,
};
use graphcast::gossip_agent::message_typing::GraphcastMessage;
use graphcast::graphql::client_network::query_network_subgraph;
use graphcast::Sender;
use num_bigint::BigUint;
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};

use graphcast::gossip_agent::waku_handling::generate_pubsub_topics;
use graphcast::gossip_agent::{GossipAgent, NETWORK_SUBGRAPH};
use graphcast::graphql::query_graph_node_poi;
use std::{thread::sleep, time::Duration};
use waku::WakuPubSubTopic;

#[macro_use]
extern crate partial_application;

pub static GOSSIP_AGENT: OnceCell<GossipAgent> = OnceCell::new();

#[derive(Clone, Debug)]
pub struct Attestation {
    pub npoi: String,
    pub stake_weight: BigUint,
    pub senders: Option<Vec<String>>,
}

impl Attestation {
    pub fn new(npoi: String, stake_weight: BigUint, senders: Option<Vec<String>>) -> Self {
        Attestation {
            npoi,
            stake_weight,
            senders,
        }
    }

    pub fn update(base: &Self, address: String, stake: BigUint) -> Self {
        let senders = Some([base.senders.as_ref().unwrap().clone(), vec![address]].concat());
        Self::new(
            base.npoi.clone(),
            base.stake_weight.clone() + stake,
            senders,
        )
    }
}

fn clone_blocks(
    block_number: u64,
    blocks: &HashMap<u64, Vec<Attestation>>,
    ipfs_hash: String,
    stake: BigUint,
    address: String,
) -> HashMap<u64, Vec<Attestation>> {
    let mut blocks_clone: HashMap<u64, Vec<Attestation>> = HashMap::new();
    blocks_clone.extend(blocks.clone());
    blocks_clone.insert(
        block_number,
        vec![Attestation::new(ipfs_hash, stake, Some(vec![address]))],
    );
    blocks_clone
}

type RemoteAttestationsMap = HashMap<String, HashMap<u64, Vec<Attestation>>>;
type LocalAttestationsMap = HashMap<String, HashMap<u64, Attestation>>;

pub static REMOTE_ATTESTATIONS: OnceCell<Arc<Mutex<RemoteAttestationsMap>>> = OnceCell::new();
pub static LOCAL_ATTESTATIONS: OnceCell<Arc<Mutex<LocalAttestationsMap>>> = OnceCell::new();

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

    let radio_handler = |msg: Result<(Sender, GraphcastMessage), anyhow::Error>| match msg {
        Ok((sender, msg)) => {
            println!("Decoded valid message: {:?}", msg);

            let (sender_address, sender_stake) = match sender {
                Sender::Indexer { address, stake } => (address, stake),
            };

            let mut remote_attestations = REMOTE_ATTESTATIONS.get().unwrap().lock().unwrap();
            let blocks = remote_attestations.get(&msg.identifier);

            match blocks {
                Some(blocks) => {
                    // Already has attestations for that block
                    let attestations = blocks.get(&msg.block_number);
                    match attestations {
                        Some(attestations) => {
                            let attestations_clone: Vec<Attestation> = Vec::new();
                            let mut attestations_clone =
                                [attestations_clone, attestations.to_vec()].concat();

                            let existing_attestation =
                                attestations_clone.iter().find(|a| a.npoi == msg.content);

                            match existing_attestation {
                                Some(existing_attestation) => {
                                    if existing_attestation
                                        .senders
                                        .as_ref()
                                        .unwrap()
                                        .contains(&sender_address)
                                    {
                                        println!("{}", "There is already an attestation from this address. Skipping...".yellow());
                                    } else {
                                        let updated_attestation = Attestation::update(
                                            existing_attestation,
                                            sender_address.clone(),
                                            sender_stake.clone(),
                                        );
                                        // Remove old
                                        if let Some(index) = attestations_clone
                                            .iter()
                                            .position(|a| a.npoi == existing_attestation.npoi)
                                        {
                                            attestations_clone.swap_remove(index);
                                        }
                                        // Add new
                                        attestations_clone.push(updated_attestation);

                                        // Update map
                                        let blocks_clone = clone_blocks(
                                            msg.block_number,
                                            blocks,
                                            msg.content,
                                            sender_stake,
                                            sender_address,
                                        );
                                        remote_attestations.insert(msg.identifier, blocks_clone);
                                    }
                                }
                                None => {
                                    let blocks_clone = clone_blocks(
                                        msg.block_number,
                                        blocks,
                                        msg.content,
                                        sender_stake,
                                        sender_address,
                                    );
                                    remote_attestations.insert(msg.identifier, blocks_clone);
                                }
                            }
                        }
                        None => {
                            let blocks_clone = clone_blocks(
                                msg.block_number,
                                blocks,
                                msg.content,
                                sender_stake,
                                sender_address,
                            );
                            remote_attestations.insert(msg.identifier, blocks_clone);
                        }
                    }
                }
                None => {
                    let blocks_clone = clone_blocks(
                        msg.block_number,
                        &HashMap::new(),
                        msg.content,
                        sender_stake,
                        sender_address,
                    );
                    remote_attestations.insert(msg.identifier, blocks_clone);
                }
            }
        }
        Err(err) => {
            println!("{}", err);
        }
    };

    let radio_handler = Arc::new(Mutex::new(radio_handler));
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
            let attestation_block = &(compare_block - wait_block_duration);

            let remote = REMOTE_ATTESTATIONS.get().unwrap().lock().unwrap();
            let local = LOCAL_ATTESTATIONS.get().unwrap().lock().unwrap();

            // Iterate & compare
            for (ipfs_hash, blocks) in local.iter() {
                let attestations = blocks.get(attestation_block);
                match attestations {
                    Some(local_attestation) => {
                        let remote_blocks = remote.get(ipfs_hash);
                        match remote_blocks {
                            Some(remote_blocks) => {
                                // Unwrapping because we're sure that if there's an entry for the subgraph there will also be at least one attestation
                                let remote_attestations =
                                    remote_blocks.get(attestation_block).unwrap();

                                let remote_attestations_clone: Vec<Attestation> = Vec::new();
                                let mut remote_attestations_clone =
                                    [remote_attestations_clone, remote_attestations.to_vec()]
                                        .concat();

                                // Sort remote
                                remote_attestations_clone.sort_by(|a, b| {
                                    a.stake_weight.partial_cmp(&b.stake_weight).unwrap()
                                });

                                let most_attested_npoi = &remote_attestations.last().unwrap().npoi;
                                if most_attested_npoi == &local_attestation.npoi {
                                    println!(
                                        "{}",
                                        format!(
                                            "POIs match for subgraph {} on block {}!",
                                            ipfs_hash, attestation_block
                                        )
                                        .green()
                                        .bold()
                                    );
                                } else {
                                    println!(
                                        "{}",
                                        format!(
                                            "POIs don't match for subgraph {} on block {}!",
                                            ipfs_hash, attestation_block
                                        )
                                        .red()
                                        .bold()
                                    );
                                    // Take some action - send alert, close allocations, etc
                                }
                            }
                            None => {
                                println!("{}", format!("No attestations for subgraph {} on block {} found in remote attestations store. Continuing...", ipfs_hash, attestation_block, ).yellow());
                            }
                        }
                    }
                    None => {
                        println!("{}", format!("No attestation for subgraph {} on block {} found in local attestations store. Continuing...", ipfs_hash, attestation_block, ).yellow());
                    }
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
                        senders: None,
                    };

                    // Save message to local attestations before sending
                    let mut local_attestations = LOCAL_ATTESTATIONS.get().unwrap().lock().unwrap();
                    let blocks = local_attestations.get(ipfs_hash);

                    match blocks {
                        Some(blocks) => {
                            let mut blocks_clone: HashMap<u64, Attestation> = HashMap::new();
                            blocks_clone.extend(blocks.clone());
                            blocks_clone.insert(block_number, attestation);
                            local_attestations.insert(ipfs_hash.to_string(), blocks_clone);
                        }
                        None => {
                            let mut blocks_clone: HashMap<u64, Attestation> = HashMap::new();
                            blocks_clone.insert(block_number, attestation);
                            local_attestations.insert(ipfs_hash.to_string(), blocks_clone);
                        }
                    }

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
