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

/// Query network subgraph for Graph account
pub async fn query_graph_account(
    url: &str,
    operator: &str,
    account: &str,
) -> Result<Account, QueryError> {
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

/// Query network subgraph for subgraph ownership account
/// There could be operator relationship between subgraph owner and registered operator
pub async fn owned_subgraphs(url: &str, account: &str) -> Result<Vec<String>, QueryError> {
    let variables: graph_account::Variables = graph_account::Variables {
        // Do not supply operator address if operator is already a graph account
        operator_addr: vec![],
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
            "Missing response data from network subgraph for account {}",
            account
        ))
    })?;

    let subgraphs: Vec<String> = data
        .graph_accounts
        .first()
        .map(|x| {
            x.subgraphs
                .iter()
                .map(|s| s.id.clone())
                .collect::<Vec<String>>()
        })
        .ok_or_else(|| {
            QueryError::ParseResponseError(String::from(
                "Network subgraph does not have a match for graph account",
            ))
        })?;

    Ok(subgraphs)
}

/// Query network subgraph for subgraph ownership account
/// There could be operator relationship between subgraph owner and registered operator
pub async fn subgraph_hash_by_id(
    url: &str,
    account: &str,
    subgraph_id: &str,
) -> Result<Vec<String>, QueryError> {
    let variables: graph_account::Variables = graph_account::Variables {
        // Do not supply operator address if operator is already a graph account
        operator_addr: vec![],
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
            "Missing response data from network subgraph for account {}",
            account
        ))
    })?;

    let hashes: Vec<String> = data
        .graph_accounts
        .first()
        .map(|x| {
            x.subgraphs
                .iter()
                .find_map(|s| {
                    if s.id.clone() == subgraph_id {
                        s.linked_entity.as_ref().map(|e| {
                            e.versions
                                .iter()
                                .map(|v| v.subgraph_deployment.ipfs_hash.clone())
                                .collect::<Vec<String>>()
                        })
                    } else {
                        None
                    }
                })
                .unwrap_or_default()
        })
        .ok_or_else(|| {
            QueryError::ParseResponseError(String::from(
                "Network subgraph does not have a match for graph account",
            ))
        })?;

    Ok(hashes)
}

#[cfg(test)]
mod tests {

    use super::*;

    #[tokio::test]
    async fn test_owned_subgraphs() {
        let network_subgraph =
            "https://api.thegraph.com/subgraphs/name/graphprotocol/graph-network-goerli";
        let account = "0xe9a1cabd57700b17945fd81feefba82340d9568f";
        let owned_subgraphs = owned_subgraphs(network_subgraph, account).await;

        assert!(owned_subgraphs.is_ok());
        // Current subgraph number
        assert!(owned_subgraphs.unwrap().len() > 5);
    }
    #[tokio::test]
    async fn test_subgraph_linked() {
        let network_subgraph =
            "https://api.thegraph.com/subgraphs/name/graphprotocol/graph-network-mainnet";
        let account = "0x00000444e5a1a667663b0adfd853e8efa0470698";
        let subgraph_id = "0x00000444e5a1a667663b0adfd853e8efa0470698-0";
        let subgraph_hashes = subgraph_hash_by_id(network_subgraph, account, subgraph_id)
            .await
            .unwrap();

        assert!(subgraph_hashes.contains(&String::from(
            "QmQQeCUjemEf6urSR5SUvvdRTn9ZXdctHwuxjPJoFJD6wR"
        )));
        assert!(subgraph_hashes.contains(&String::from(
            "QmXpLT9V82VMYbBCDKTiTAEpG3g6CD3DygRhxmUTiDu9eF"
        )));
        assert!(subgraph_hashes.contains(&String::from(
            "QmfDJFYaDX7BdwT6rYa8Bx71vPjTueUVDN99pdwFgysDiZ"
        )));
    }
}
