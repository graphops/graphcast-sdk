use graphcast::graphql::QueryError;
use graphql_client::{GraphQLQuery, Response};
use serde_derive::{Deserialize, Serialize};

/// Derived GraphQL Query to Proof of Indexing
//TODO: refactor ProofOfIndexing typing for build query
#[derive(GraphQLQuery, Serialize, Deserialize, Debug)]
#[graphql(
    schema_path = "examples/poi-crosschecker/src/graphql/schema_proof_of_indexing.graphql",
    query_path = "examples/poi-crosschecker/src/graphql/query_proof_of_indexing.graphql",
    response_derives = "Debug, Serialize, Deserialize"
)]
pub struct ProofOfIndexing;

/// Query graph node for Proof of Indexing
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

/// Construct GraphQL variables and parse result for Proof of Indexing
pub async fn query_graph_node_poi(
    graph_node_endpoint: String,
    ipfs_hash: String,
    block_hash: String,
    block_number: i64,
) -> Result<String, QueryError> {
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
        match data.proof_of_indexing {
            Some(poi) => Ok(poi),
            _ => Err(QueryError::EmptyResponseError),
        }
    } else {
        Err(QueryError::EmptyResponseError)
    }
}
