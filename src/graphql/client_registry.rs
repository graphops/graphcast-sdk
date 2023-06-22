use graphql_client::{GraphQLQuery, Response};
use serde_derive::{Deserialize, Serialize};
use tracing::{trace, warn};

use super::QueryError;

/// Derived Indexer
#[derive(GraphQLQuery, Serialize, Deserialize, Debug)]
#[graphql(
    schema_path = "src/graphql/schema_registry.graphql",
    query_path = "src/graphql/query_registry.graphql",
    response_derives = "Debug, Serialize, Deserialize"
)]
pub struct SetGraphcastIds;

/// Query registry subgraph endpoint for resolving Graphcast ID and indexer address
pub async fn perform_graphcast_id_indexer_query(
    registry_subgraph_endpoint: String,
    variables: set_graphcast_ids::Variables,
) -> Result<reqwest::Response, reqwest::Error> {
    let request_body = SetGraphcastIds::build_query(variables);
    let client = reqwest::Client::new();
    client
        .post(registry_subgraph_endpoint)
        .json(&request_body)
        .send()
        .await?
        .error_for_status()
}

/// Construct GraphQL variables and parse result for indexer address
pub async fn query_registry(
    registry_subgraph_endpoint: String,
    wallet_address: String,
) -> Result<String, QueryError> {
    let variables: set_graphcast_ids::Variables = set_graphcast_ids::Variables {
        address: wallet_address.clone(),
    };
    let queried_result =
        perform_graphcast_id_indexer_query(registry_subgraph_endpoint.clone(), variables).await?;
    trace!(
        result = tracing::field::debug(&queried_result),
        "Query result for registry indexer"
    );
    if !&queried_result.status().is_success() {
        warn!(
            result = tracing::field::debug(&queried_result),
            "Unsuccessful query"
        );
    }
    let response_body: Response<set_graphcast_ids::ResponseData> = queried_result.json().await?;
    if let Some(data) = response_body.data {
        data.graphcast_ids
            .first()
            .map(|event| event.indexer.clone())
            .ok_or(QueryError::ParseResponseError(format!(
                "No indexer data queried from registry for GraphcastID: {wallet_address}"
            )))
    } else {
        Err(QueryError::ParseResponseError(format!(
            "No response data from registry for GraphcastID: {wallet_address}"
        )))
    }
}
