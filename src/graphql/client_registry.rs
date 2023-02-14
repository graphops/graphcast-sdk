use anyhow::anyhow;
use graphql_client::{GraphQLQuery, Response};
use serde_derive::{Deserialize, Serialize};

/// Derived Graph Account
#[derive(GraphQLQuery, Serialize, Deserialize, Debug)]
#[graphql(
    schema_path = "src/graphql/schema_registry.graphql",
    query_path = "src/graphql/query_registry.graphql",
    response_derives = "Debug, Serialize, Deserialize"
)]
pub struct GraphAccount;

/// Query registry subgraph endpoint for resolving Graphcast ID and indexer address
pub async fn perform_operator_indexer_query(
    registry_subgraph_endpoint: String,
    variables: graph_account::Variables,
) -> Result<reqwest::Response, reqwest::Error> {
    let request_body = GraphAccount::build_query(variables);
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
    operator_address: String,
) -> Result<String, anyhow::Error> {
    let variables: graph_account::Variables = graph_account::Variables {
        address: operator_address,
    };
    let queried_result =
        perform_operator_indexer_query(registry_subgraph_endpoint, variables).await?;

    let response_body: Response<graph_account::ResponseData> = queried_result.json().await?;

    if let Some(data) = response_body.data {
        let account = data
            .graph_account
            .and_then(|x| x.gossip_operator_of)
            .map(|x| x.id);
        match account {
            Some(a) => Ok(a),
            None => Err(anyhow!("Empty graphAccount data queried from registry")),
        }
    } else {
        Err(anyhow!("No response data from registry"))
    }
}
