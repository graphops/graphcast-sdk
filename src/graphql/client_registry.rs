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

pub async fn query_registry_indexer(
    registry_subgraph_endpoint: String,
    graphcast_id_address: String,
) -> Result<String, QueryError> {
    let variables: set_graphcast_ids::Variables = set_graphcast_ids::Variables {
        address: graphcast_id_address.clone(),
    };

    let queried_result =
        perform_graphcast_id_indexer_query(registry_subgraph_endpoint.clone(), variables).await?;

    if !queried_result.status().is_success() {
        warn!(
            result = tracing::field::debug(&queried_result.status()),
            "Unsuccessful query"
        );
    }

    let bytes = queried_result.bytes().await?;
    let cloned_bytes = bytes.clone();

    let raw_response_body = std::str::from_utf8(&cloned_bytes).unwrap_or("Invalid UTF-8");
    trace!("Raw response body: {}", raw_response_body);

    let response_body: Response<set_graphcast_ids::ResponseData> = serde_json::from_slice(&bytes)
        .map_err(|err| {
        QueryError::ParseResponseError(format!("Error parsing response body as JSON: {err}"))
    })?;

    trace!("Response body {:?}", response_body);

    if let Some(data) = response_body.data {
        data.graphcast_ids
            .first()
            .map(|event| event.indexer.clone())
            .ok_or(QueryError::ParseResponseError(format!(
                "No indexer data queried from registry for GraphcastID: {graphcast_id_address}"
            )))
    } else {
        Err(QueryError::ParseResponseError(format!(
            "No response data from registry for graphcastID {graphcast_id_address}"
        )))
    }
}
