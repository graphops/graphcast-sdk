use graphql_client::{GraphQLQuery, Response};
use serde_derive::{Deserialize, Serialize};
use tracing::warn;

use super::QueryError;

/// Derived Indexer
#[derive(GraphQLQuery, Serialize, Deserialize, Debug)]
#[graphql(
    schema_path = "src/graphql/schema_registry.graphql",
    query_path = "src/graphql/query_registry.graphql",
    response_derives = "Debug, Serialize, Deserialize"
)]
pub struct Indexers;

/// Query registry subgraph endpoint for resolving Graphcast ID and indexer address
pub async fn perform_graphcast_id_indexer_query(
    registry_subgraph_endpoint: String,
    variables: indexers::Variables,
) -> Result<reqwest::Response, reqwest::Error> {
    let request_body = Indexers::build_query(variables);
    let client = reqwest::Client::new();
    client
        .post(registry_subgraph_endpoint)
        .json(&request_body)
        .send()
        .await?
        .error_for_status()
}

/// Construct GraphQL variables and parse result for indexer address
pub async fn query_registry_indexer(
    registry_subgraph_endpoint: String,
    graphcast_id_address: String,
) -> Result<String, QueryError> {
    let variables: indexers::Variables = indexers::Variables {
        address: graphcast_id_address.clone(),
    };
    let queried_result =
        perform_graphcast_id_indexer_query(registry_subgraph_endpoint, variables).await?;
    if !&queried_result.status().is_success() {
        warn!("Unsuccessful query detail: {:#?}", queried_result);
    }
    let response_body: Response<indexers::ResponseData> = queried_result.json().await?;
    if let Some(data) = response_body.data {
        data.indexers
            .first()
            .map(|indexer| indexer.id.clone())
            .ok_or(QueryError::ParseResponseError(format!(
                "No indexer data queried from registry for GraphcastID: {graphcast_id_address}"
            )))
    } else {
        Err(QueryError::ParseResponseError(format!(
            "No response data from registry for graphcastID {graphcast_id_address}"
        )))
    }
}
