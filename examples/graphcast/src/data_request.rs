use waku::{Encoding, WakuPubSubTopic};

use crate::{
    client_network::{
        query_indexer_allocations, query_indexer_stake, query_stake_minimum_requirement,
    },
    client_registry::query_registry_indexer,
};

pub async fn testing_functionalities() -> String {
    let test_topic = String::from("QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U");
    let network_subgraph = String::from("https://gateway.testnet.thegraph.com/network");
    let registry_subgraph =
        String::from("https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test");

    let received_from_address = String::from("0x2bc5349585cbbf924026d25a520ffa9e8b51a39b");
    let indexer_address = query_registry_indexer(registry_subgraph, received_from_address).await;
    println!("Indexer address:  {}", indexer_address);

    let indexer_stake =
        query_indexer_stake(network_subgraph.clone(), indexer_address.clone()).await;
    println!("Indexer stake:  {}", indexer_stake);
    let indexer_allocations_temp =
        query_indexer_allocations(network_subgraph.clone(), indexer_address.clone()).await;
    //Temp: test topic for local poi
    let indexer_allocations = [String::from(&test_topic)];
    println!(
        "Indexer allocations: \n{:?}\nAdd test topic:\n{:?}",
        indexer_allocations_temp, indexer_allocations,
    );
    let minimum_req =
        query_stake_minimum_requirement(network_subgraph.clone(), indexer_address.clone()).await;
    println!("fetch minimum indexer requirement:  {}", minimum_req);

    indexer_stake
}

//TODO: refactor topic generation
pub async fn generate_pubsub_topics(
    app_name: String,
    items: Vec<String>,
) -> Vec<Option<WakuPubSubTopic>> {
    items
        .into_iter()
        .map(|hash| {
            let borrowed_hash: &str = &hash;
            let topic = app_name.clone() + "-poi-crosschecker-" + borrowed_hash;

            Some(WakuPubSubTopic {
                topic_name: topic,
                encoding: Encoding::Proto,
            })
        })
        .collect::<Vec<Option<WakuPubSubTopic>>>()
}
