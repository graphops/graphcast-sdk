use graphql_client::GraphQLQuery;
use num_bigint::BigUint;
use serde_derive::{Deserialize, Serialize};

use crate::query_network::indexer::Variables;
use crate::query_network::Indexer as indexer_query;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SubgraphDeployment {
    #[serde(rename = "ipfsHash")]
    pub ipfs_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Allocation {
    #[serde(rename = "subgraphDeployment")]
    pub subgraph_deployment: SubgraphDeployment,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IndexerJSON {
    #[serde(rename = "stakedTokens")]
    staked_tokens: String,
    allocations: Vec<Allocation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GraphNetworkJSON {
    #[serde(rename = "minimumIndexerStake")]
    pub minimum_indexer_stake: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IndexerData {
    pub indexer: IndexerJSON,
    #[serde(rename = "graphNetwork")]
    pub graph_network: GraphNetworkJSON,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IndexerResponse {
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
    indexer_address: String,
) -> Result<IndexerResponse, anyhow::Error> {
    let variables: Variables = Variables {
        address: indexer_address,
    };
    let request_body = indexer_query::build_query(variables);
    let client = reqwest::Client::new();
    let res = client
        .post(graph_network_endpoint)
        .json(&request_body)
        .send()
        .await?
        .text()
        .await?;
    let formatted_res: IndexerResponse = serde_json::from_str(&res)?;
    Ok(formatted_res)
}

// Query indexer staking infomation, namely staked tokens and active allocations
pub async fn query_indexer_stake(
    indexer_response: &IndexerResponse,
) -> Result<BigUint, anyhow::Error> {
    let tokens = indexer_response
        .data
        .indexer
        .staked_tokens
        .parse::<BigUint>()?;
    Ok(tokens)
}

// Query indexer staking infomation, namely staked tokens and active allocations
pub async fn query_indexer_allocations(
    indexer_response: IndexerResponse,
) -> Result<Vec<String>, anyhow::Error> {
    let allocations = indexer_response
        .data
        .indexer
        .allocations
        .into_iter()
        .map(|a| a.subgraph_deployment.ipfs_hash)
        .collect::<Vec<String>>();
    Ok(allocations)
}

// Query indexer staking infomation, namely staked tokens and active allocations
pub async fn query_stake_minimum_requirement(
    indexer_response: &IndexerResponse,
) -> Result<BigUint, anyhow::Error> {
    let min_req: BigUint = indexer_response
        .data
        .graph_network
        .minimum_indexer_stake
        .parse::<BigUint>()?;
    Ok(min_req)
}
