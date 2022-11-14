use graphql_client::GraphQLQuery;
use serde_derive::{Deserialize, Serialize};

use crate::query_registry::graph_account::Variables;
use crate::query_registry::GraphAccount as registry_query;

#[derive(Debug, Deserialize, Serialize)]
struct ID {
    pub id: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct GraphAccountOpened {
    #[serde(rename = "gossipOperatorOf")]
    pub gossip_operator_of: ID,
}

#[derive(Debug, Deserialize, Serialize)]
struct GraphAccountData {
    #[serde(rename = "graphAccount")]
    pub graph_account: GraphAccountOpened,
}

#[derive(Debug, Deserialize, Serialize)]
struct GraphAccountResponse {
    pub data: GraphAccountData,
}

#[derive(GraphQLQuery, Serialize, Deserialize, Debug)]
#[graphql(
    schema_path = "src/schema_registry.graphql",
    query_path = "src/query_registry.graphql",
    response_derives = "Debug, Serialize, Deserialize"
)]
pub struct GraphAccount;

pub async fn perform_operator_indexer_query(
    registry_subgraph_endpoint: String,
    variables: Variables,
) -> std::string::String {
    let request_body = registry_query::build_query(variables);
    let client = reqwest::Client::new();
    client
        .post(registry_subgraph_endpoint)
        .json(&request_body)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
}

pub async fn query_registry_indexer(
    registry_subgraph_endpoint: String,
    operator_address: String,
) -> String {
    let variables: Variables = Variables {
        address: operator_address,
    };
    let queried_result =
        &perform_operator_indexer_query(registry_subgraph_endpoint, variables).await;

    let perform_operator_indexer_query: GraphAccountResponse =
        serde_json::from_str(queried_result).unwrap();
    perform_operator_indexer_query
        .data
        .graph_account
        .gossip_operator_of
        .id
}
