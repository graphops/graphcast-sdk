use graphql_client::{GraphQLQuery, Response};
use num_bigint::ParseBigIntError;
use serde_derive::{Deserialize, Serialize};
use std::error::Error;

pub mod client_network;
pub mod client_registry;
// refactor at a later time
pub mod query_network;

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
    #[error("Failed to parse")]
    ParsingError(#[from] ParseBigIntError),
    #[error("The subgraph is in a failed state")]
    IndexingError,
    #[error("Unknown error: {0}")]
    Other(anyhow::Error),
}

//TODO: refactor ProofOfIndexing typing for build query
#[derive(GraphQLQuery, Serialize, Deserialize, Debug)]
#[graphql(
    schema_path = "src/graphql/schema_proof_of_indexing.graphql",
    query_path = "src/graphql/query_proof_of_indexing.graphql",
    response_derives = "Debug, Serialize, Deserialize"
)]
pub struct ProofOfIndexing;

pub async fn perform_proof_of_indexing(
    graph_node_endpoint: String,
    variables: proof_of_indexing::Variables,
) -> Result<reqwest::Response, reqwest::Error> {
    let request_body = ProofOfIndexing::build_query(variables);
    let client = reqwest::Client::new();
    client
        .post(graph_node_endpoint)
        .json(&request_body)
        .send()
        .await?
        .error_for_status()
}

pub async fn query_graph_node_poi(
    graph_node_endpoint: String,
    ipfs_hash: String,
    block_hash: String,
    block_number: i64,
) -> Result<proof_of_indexing::ResponseData, Box<dyn Error>> {
    let variables: proof_of_indexing::Variables = proof_of_indexing::Variables {
        subgraph: ipfs_hash.clone(),
        block_hash: block_hash.clone(),
        block_number,
        indexer: None,
    };
    let queried_result = perform_proof_of_indexing(graph_node_endpoint.clone(), variables).await?;
    // let queried_result = "{\"data\":{\"proofOfIndexing\":\"0xa6008cea5905b8b7811a68132feea7959b623188e2d6ee3c87ead7ae56dd0eae\"}}";
    let response_body: Response<proof_of_indexing::ResponseData> = queried_result.json().await?;

    if let Some(data) = response_body.data {
        Ok(data)
    } else {
        Err(format!(
            "{} failed to return data for {}",
            graph_node_endpoint, ipfs_hash
        ))?
    }
}
