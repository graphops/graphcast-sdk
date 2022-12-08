use graphql_client::{GraphQLQuery, Response};
use num_bigint::BigUint;
use num_traits::Zero;

use crate::graphql::QueryError;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/schema_network.graphql",
    query_path = "src/graphql/query_network.graphql",
    response_derives = "Debug, Serialize, Deserialize"
)]
pub struct NetworkSubgraph;

pub async fn query_network_subgraph(
    url: String,
    indexer_address: String,
) -> Result<Network, QueryError> {
    // Can refactor for all types of queries
    let variables: network_subgraph::Variables = network_subgraph::Variables {
        address: indexer_address.clone(),
    };
    let request_body = NetworkSubgraph::build_query(variables);
    let client = reqwest::Client::builder()
        .user_agent("network-subgraph")
        .build()
        .unwrap();
    let request = client.post(url.clone()).json(&request_body);
    let response = request.send().await?.error_for_status()?;
    let response_body: Response<network_subgraph::ResponseData> = response.json().await?;

    match response_body.errors.as_deref() {
        Some([]) | None => {
            println!("response body all good");
        }
        Some(errors) => {
            let e = &errors[0];
            if e.message == "indexing_error" {
                return Err(QueryError::IndexingError);
            } else {
                return Err(QueryError::Other(anyhow::anyhow!("{}", e.message)));
            }
        }
    }
    let data = if let Some(data) = response_body.data {
        data
    } else {
        return Err(QueryError::Other(anyhow::anyhow!(format!(
            "Missing response data from network subgraph for {}",
            indexer_address
        ))));
    };

    let indexer =
        data.indexer.and_then(
            |x| match Some(x.staked_tokens.parse::<BigUint>()).transpose() {
                Ok(token) => {
                    let allocations: Vec<Allocation> = x.allocations.map(|allocs| {
                        allocs
                            .iter()
                            .map(|alloc| Allocation {
                                subgraph_deployment: SubgraphDeployment {
                                    ipfs_hash: alloc.subgraph_deployment.ipfs_hash.clone(),
                                },
                            })
                            .collect::<Vec<Allocation>>()
                    })?;
                    Some(Indexer {
                        staked_tokens: token?,
                        allocations,
                    })
                }
                Err(e) => {
                    println!("Indexer not available from the network subgraph: {}", e);
                    None
                }
            },
        );

    Ok(Network {
        indexer,
        graph_network: GraphNetwork {
            minimum_indexer_stake: data
                .graph_network
                .minimum_indexer_stake
                .parse::<BigUint>()?,
        },
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    pub indexer: Option<Indexer>,
    pub graph_network: GraphNetwork,
}

impl Network {
    // Query indexer staking infomation, namely staked tokens and active allocations
    pub fn indexer_stake(&self) -> BigUint {
        self.indexer
            .as_ref()
            .map(|i| i.staked_tokens.clone())
            .unwrap_or_else(Zero::zero)
    }

    // Query indexer staking infomation, namely staked tokens and active allocations
    pub fn minimum_stake_requirement(&self) -> BigUint {
        self.graph_network.minimum_indexer_stake.clone()
    }

    // Query indexer staking infomation, namely staked tokens and active allocations
    pub fn indexer_allocations(&self) -> Vec<String> {
        self.indexer
            .as_ref()
            .map(|i| {
                i.allocations
                    .iter()
                    .map(|a| a.subgraph_deployment.ipfs_hash.clone())
                    .collect::<Vec<String>>()
            })
            .unwrap()
    }
}

// #[serde(rename = "ipfsHash")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubgraphDeployment {
    pub ipfs_hash: String,
}

// #[serde(rename = "subgraphDeployment")]
// Can later add - who else is allocating to this, allocatedTokens, signalTokens
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub subgraph_deployment: SubgraphDeployment,
}

// #[serde(rename = "stakedTokens")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Indexer {
    staked_tokens: BigUint,
    allocations: Vec<Allocation>,
}

// #[serde(rename = "minimumIndexerStake")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphNetwork {
    pub minimum_indexer_stake: BigUint,
}
