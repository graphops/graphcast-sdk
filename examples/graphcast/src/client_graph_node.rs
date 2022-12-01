use graphql_client::GraphQLQuery;
use serde_derive::{Deserialize, Serialize};
use std::error::Error;

use crate::query_proof_of_indexing::proof_of_indexing::Variables;
use crate::query_proof_of_indexing::ProofOfIndexing as proof_of_indexing_query;

#[derive(Debug, Deserialize, Serialize)]
pub struct ProofOfIndexingData {
    #[serde(rename = "proofOfIndexing")]
    pub proof_of_indexing: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProofOfIndexingResponse {
    pub data: ProofOfIndexingData,
}

//TODO: refactor ProofOfIndexing typing for build query
#[derive(GraphQLQuery, Serialize, Deserialize, Debug)]
#[graphql(
    schema_path = "src/schema_proof_of_indexing.graphql",
    query_path = "src/query_proof_of_indexing.graphql",
    response_derives = "Debug, Serialize, Deserialize"
)]
pub struct ProofOfIndexing;

pub async fn perform_proof_of_indexing(
    graph_node_endpoint: String,
    variables: Variables,
) -> Result<String, reqwest::Error> {
    let request_body = proof_of_indexing_query::build_query(variables);
    let client = reqwest::Client::new();
    client
        .post(graph_node_endpoint)
        .json(&request_body)
        .send()
        .await?
        .text()
        .await
}

pub async fn query_graph_node_poi(
    graph_node_endpoint: String,
    ipfs_hash: String,
    block_hash: String,
    block_number: i64,
) -> Result<ProofOfIndexingResponse, Box<dyn Error>> {
    let variables: Variables = Variables {
        subgraph: ipfs_hash.clone(),
        block_hash: block_hash.clone(),
        block_number,
        indexer: None,
    };
    let queried_result = &perform_proof_of_indexing(graph_node_endpoint, variables).await?;
    // let queried_result = "{\"data\":{\"proofOfIndexing\":\"0xa6008cea5905b8b7811a68132feea7959b623188e2d6ee3c87ead7ae56dd0eae\"}}";
    println!("queried result {}", queried_result);
    Ok(serde_json::from_str(queried_result)?)
}
