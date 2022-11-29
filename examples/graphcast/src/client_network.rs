use graphql_client::GraphQLQuery;
use num_bigint::BigUint;
use serde_derive::{Deserialize, Serialize};

use crate::query_network::indexer::Variables;
use crate::query_network::Indexer as indexer_query;

#[derive(Debug, Deserialize, Serialize)]
struct SubgraphDeployment {
    #[serde(rename = "ipfsHash")]
    pub ipfs_hash: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct Allocation {
    #[serde(rename = "subgraphDeployment")]
    pub subgraph_deployment: SubgraphDeployment,
}

#[derive(Debug, Deserialize, Serialize)]
struct IndexerJSON {
    #[serde(rename = "stakedTokens")]
    staked_tokens: String,
    allocations: Vec<Allocation>,
}

#[derive(Debug, Deserialize, Serialize)]
struct GraphNetworkJSON {
    #[serde(rename = "minimumIndexerStake")]
    pub minimum_indexer_stake: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct IndexerData {
    pub indexer: IndexerJSON,
    #[serde(rename = "graphNetwork")]
    pub graph_network: GraphNetworkJSON,
}

#[derive(Debug, Deserialize, Serialize)]
struct IndexerResponse {
    pub data: IndexerData,
}

#[derive(GraphQLQuery, Serialize, Deserialize, Debug)]
#[graphql(
    schema_path = "src/schema_network.graphql",
    query_path = "src/query_network.graphql",
    response_derives = "Debug, Serialize, Deserialize"
)]
pub struct Indexer;

pub async fn perform_indexer_query(
    graph_network_endpoint: String,
    variables: Variables,
) -> Result<String, anyhow::Error> {
    let request_body = indexer_query::build_query(variables);
    let client = reqwest::Client::new();
    let res = client
        .post(graph_network_endpoint)
        .json(&request_body)
        .send()
        .await?
        .text()
        .await?;
    Ok(res)
}

// Query indexer staking infomation, namely staked tokens and active allocations
pub async fn query_indexer_stake(
    graph_network_endpoint: String,
    indexer_address: String,
) -> Result<BigUint, anyhow::Error> {
    let variables: Variables = Variables {
        address: indexer_address,
    };
    let queried_result = perform_indexer_query(graph_network_endpoint, variables).await?;

    let perform_indexer_query: IndexerResponse = serde_json::from_str(&queried_result)?;
    let tokens = perform_indexer_query
        .data
        .indexer
        .staked_tokens
        .parse::<BigUint>()?;
    Ok(tokens)
}

// Query indexer staking infomation, namely staked tokens and active allocations
pub async fn query_indexer_allocations(
    graph_network_endpoint: String,
    indexer_address: String,
) -> Result<Vec<String>, anyhow::Error> {
    let variables: Variables = Variables {
        address: indexer_address,
    };
    let queried_result = perform_indexer_query(graph_network_endpoint, variables).await?;

    let perform_indexer_query: IndexerResponse = serde_json::from_str(&queried_result)?;
    let allocations = perform_indexer_query
        .data
        .indexer
        .allocations
        .into_iter()
        .map(|a| a.subgraph_deployment.ipfs_hash)
        .collect();
    Ok(allocations)
}

// Query indexer staking infomation, namely staked tokens and active allocations
pub async fn query_stake_minimum_requirement(
    graph_network_endpoint: String,
) -> Result<BigUint, anyhow::Error> {
    let variables: Variables = Variables {
        address: "".to_string(),
    };
    let queried_result = perform_indexer_query(graph_network_endpoint, variables).await?;

    let perform_indexer_query: IndexerResponse = serde_json::from_str(&queried_result)?;
    let min_req: BigUint = perform_indexer_query
        .data
        .graph_network
        .minimum_indexer_stake
        .parse::<BigUint>()?;
    Ok(min_req)
}
