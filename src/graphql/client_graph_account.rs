use crate::{graphql::QueryError, Account};
use graphql_client::{GraphQLQuery, Response};
use tracing::trace;

/// Derived GraphQL Query to Network Subgraph
#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/schema_graph_account.graphql",
    query_path = "src/graphql/query_graph_account.graphql",
    response_derives = "Debug, Serialize, Deserialize"
)]
pub struct GraphAccount;

/// Query network subgraph for Network data
/// Contains indexer address, stake, allocations
/// and graph network minimum indexer stake requirement
pub async fn query_graph_account(
    url: &str,
    operator: &str,
    account: &str,
) -> Result<Account, QueryError> {
    // Can refactor for all types of queries
    let variables: graph_account::Variables = graph_account::Variables {
        // Do not supply operator address if operator is already a graph account
        operator_addr: if operator == account {
            vec![]
        } else {
            vec![operator.to_string()]
        },
        account_addr: account.to_string(),
    };
    let request_body = GraphAccount::build_query(variables);
    let client = reqwest::Client::builder()
        .user_agent("network-subgraph")
        .build()?;
    let request = client.post(url).json(&request_body);
    let response = request.send().await?.error_for_status()?;
    let response_body: Response<graph_account::ResponseData> = response.json().await?;
    trace!(
        result = tracing::field::debug(&response_body),
        "Query result for graph network account"
    );
    if let Some(errors) = response_body.errors.as_deref() {
        let e = &errors[0];
        if e.message == "indexing_error" {
            return Err(QueryError::IndexingError);
        } else {
            return Err(QueryError::Other(anyhow::anyhow!("{}", e.message)));
        }
    }
    let data = response_body.data.ok_or_else(|| {
        QueryError::ParseResponseError(format!(
            "Missing response data from network subgraph for account {} with agent {}",
            account, operator
        ))
    })?;

    let agent: String = if operator == account {
        account.to_string()
    } else {
        data.graph_accounts
            .first()
            .and_then(|x| x.operators.first().map(|x| x.id.clone()))
            .ok_or_else(|| {
                QueryError::ParseResponseError(String::from(
                    "Network subgraph does not have a match for agent account and graph account",
                ))
            })?
    };

    let account: String = data
        .graph_accounts
        .first()
        .map(|x| x.id.clone())
        .ok_or_else(|| {
            QueryError::ParseResponseError(String::from(
                "Network subgraph does not have a match for graph account",
            ))
        })?;

    let account = Account { agent, account };

    Ok(account)
}
