use graphql_client::{GraphQLQuery, Response};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

use poi_radio::{BlockPointer, NetworkName, SubgraphStatus};
// Maybe later on move graphql to SDK as the queries are pretty standarded
use graphcast_sdk::graphql::QueryError;

#[derive(GraphQLQuery, Serialize, Deserialize, Debug)]
#[graphql(
    schema_path = "src/graphql/schema_graph_node.graphql",
    query_path = "src/graphql/query_indexing_statuses.graphql",
    response_derives = "Debug, Serialize, Deserialize",
    normalization = "rust"
)]
pub struct IndexingStatuses;

#[derive(GraphQLQuery, Serialize, Deserialize, Debug)]
#[graphql(
    schema_path = "src/graphql/schema_graph_node.graphql",
    query_path = "src/graphql/query_block_hash_from_number.graphql",
    response_derives = "Debug, Serialize, Deserialize"
)]
pub struct BlockHashFromNumber;

/// Query graph node for Indexing Statuses
pub async fn perform_indexing_statuses(
    graph_node_endpoint: String,
    variables: indexing_statuses::Variables,
) -> Result<reqwest::Response, reqwest::Error> {
    let request_body = IndexingStatuses::build_query(variables);
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
pub async fn update_network_chainheads(
    graph_node_endpoint: String,
    network_map: &mut HashMap<NetworkName, BlockPointer>,
) -> Result<HashMap<String, SubgraphStatus>, QueryError> {
    let variables: indexing_statuses::Variables = indexing_statuses::Variables {};
    let queried_result = perform_indexing_statuses(graph_node_endpoint.clone(), variables).await?;
    let response_body: Response<indexing_statuses::ResponseData> = queried_result.json().await?;

    // subgraph (network, latest blocks)
    let mut subgraph_network_blocks: HashMap<String, SubgraphStatus> = HashMap::new();

    let updated_networks = response_body
        .data
        .map(|data| {
            data.indexing_statuses
                .into_iter()
                .map(|status| {
                    status
                        .chains
                        .into_iter()
                        .map(|chain| {
                            let network_name = chain.network.clone();
                            let _chainhead_block = chain
                                .chain_head_block
                                .map(|blk| BlockPointer {
                                    hash: blk.hash,
                                    number: blk.number.as_str().parse::<u64>().unwrap_or_default(),
                                })
                                .map(|blk| {
                                    if let Some(block) = network_map
                                        .get_mut(&NetworkName::from_string(&network_name.clone()))
                                    {
                                        *block = blk.clone();
                                    } else {
                                        network_map
                                            .entry(NetworkName::from_string(&network_name.clone()))
                                            .or_insert(blk.clone());
                                    };
                                    blk
                                });

                            let _latest_block = chain
                                .latest_block
                                .map(|blk| BlockPointer {
                                    hash: blk.hash,
                                    number: blk.number.as_str().parse::<u64>().unwrap_or_default(),
                                })
                                .map(|blk| {
                                    subgraph_network_blocks
                                        .entry(status.subgraph.clone())
                                        .or_insert(SubgraphStatus {
                                            network: chain.network.clone(),
                                            block: blk.clone(),
                                        });
                                    blk
                                });
                            network_name
                        })
                        .collect::<String>()
                })
                .collect::<Vec<String>>()
        })
        .ok_or(QueryError::IndexingError);
    debug!("Updated networks: {:#?}", updated_networks);
    Ok(subgraph_network_blocks)
}

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
    block_number: i64,
) -> Result<String, QueryError> {
    let variables: block_hash_from_number::Variables = block_hash_from_number::Variables {
        network,
        block_number,
    };
    let queried_result =
        perform_block_hash_from_number(graph_node_endpoint.clone(), variables).await?;
    let response_body: Response<block_hash_from_number::ResponseData> =
        queried_result.json().await?;

    if let Some(data) = response_body.data {
        match data.block_hash_from_number {
            Some(hash) => Ok(hash),
            _ => Err(QueryError::EmptyResponseError),
        }
    } else {
        Err(QueryError::EmptyResponseError)
    }
}
