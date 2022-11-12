use graphql_client::{GraphQLQuery};
use serde_derive::{Deserialize, Serialize};

use crate::query_proof_of_indexing::proof_of_indexing::Variables;
use crate::query_proof_of_indexing::ProofOfIndexing as proof_of_indexing_query;

#[derive(Debug, Deserialize, Serialize)]
pub struct ProofOfIndexingData {
    #[serde(rename = "proofOfIndexing")]
    pub proof_of_indexing: String
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProofOfIndexingResponse {
    pub data: ProofOfIndexingData
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
    graph_node_endpoint: String, variables: Variables,
) -> std::string::String  {
    let request_body = proof_of_indexing_query::build_query(variables);
    let client = reqwest::Client::new();
    client
        .post(graph_node_endpoint)
        .json(&request_body)
        .send()
        .await.unwrap().text().await.unwrap()
}

pub async fn query_graph_node_poi(
    graph_node_endpoint: String,
    ipfs_hash: String,
    block_hash: String,
    block_number: u32,
) -> Result<ProofOfIndexingResponse, serde_json::Error> {
    let variables: Variables = Variables{ subgraph: ipfs_hash.clone(), block_hash: block_hash.clone(), block_number: block_number.into(), indexer: None};
    let queried_result = &perform_proof_of_indexing(graph_node_endpoint, variables).await;

    let proof_of_indexing = serde_json::from_str(queried_result); 

    proof_of_indexing
}
