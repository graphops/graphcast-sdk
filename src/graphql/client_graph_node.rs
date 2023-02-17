use crate::graphql::QueryError;
use graphql_client::{GraphQLQuery, Response};
use serde_derive::{Deserialize, Serialize};

#[derive(GraphQLQuery, Serialize, Deserialize, Debug)]
#[graphql(
    schema_path = "src/graphql/schema_graph_node.graphql",
    query_path = "src/graphql/query_block_hash_from_number.graphql",
    response_derives = "Debug, Serialize, Deserialize"
)]
pub struct BlockHashFromNumber;

/// Query graph node for Block hash
pub async fn perform_block_hash_from_number(
    graph_node_endpoint: String,
    variables: block_hash_from_number::Variables,
) -> Result<reqwest::Response, reqwest::Error> {
    let request_body = BlockHashFromNumber::build_query(variables);
    let client = reqwest::Client::new();
    client
        .post(graph_node_endpoint)
        .json(&request_body)
        .send()
        .await?
        .error_for_status()
}

/// Construct GraphQL variables and parse result for Proof of Indexing.
/// For other radio use cases, provide a function that returns a string
pub async fn query_graph_node_network_block_hash(
    graph_node_endpoint: String,
    network: String,
    block_number: u64,
) -> Result<String, QueryError> {
    let variables: block_hash_from_number::Variables = block_hash_from_number::Variables {
        network,
        block_number: block_number.try_into().unwrap(),
    };
    let queried_result =
        perform_block_hash_from_number(graph_node_endpoint.clone(), variables).await?;
    let response_body: Response<block_hash_from_number::ResponseData> =
        queried_result.json().await?;

    if let Some(data) = response_body.data {
        match data.block_hash_from_number {
            Some(hash) => Ok(hash),
            None => Err(QueryError::EmptyResponseError(
                "No block hash from number".to_string(),
            )),
        }
    } else {
        Err(QueryError::EmptyResponseError(graph_node_endpoint))
    }
}
