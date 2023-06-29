use graphql_client::{GraphQLQuery, Response};
use num_traits::Zero;
use tracing::error;

use crate::graphql::QueryError;

use super::grt_gwei_string_to_f32;

/// Derived GraphQL Query to Network Subgraph
#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "src/graphql/schema_network.graphql",
    query_path = "src/graphql/query_network.graphql",
    response_derives = "Debug, Serialize, Deserialize"
)]
pub struct NetworkSubgraph;

/// Query network subgraph for Network data
/// Contains indexer address, stake, allocations
/// and graph network minimum indexer stake requirement
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
        .build()?;
    let request = client.post(url).json(&request_body);
    let response = request.send().await?.error_for_status()?;
    let response_body: Response<network_subgraph::ResponseData> = response.json().await?;

    if let Some(errors) = response_body.errors.as_deref() {
        let e = &errors[0];
        if e.message == "indexing_error" {
            return Err(QueryError::IndexingError);
        } else {
            return Err(QueryError::Other(anyhow::anyhow!("{}", e.message)));
        }
    }
    let data = if let Some(data) = response_body.data {
        data
    } else {
        return Err(QueryError::ParseResponseError(format!(
            "Missing response data from network subgraph for {indexer_address}"
        )));
    };

    let indexer = data.indexer.and_then(|x| {
        match Some(grt_gwei_string_to_f32(x.staked_tokens)).transpose() {
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
                error!(
                    error = tracing::field::debug(&e),
                    "Indexer not available from the network subgraph"
                );

                None
            }
        }
    });

    Ok(Network {
        indexer,
        graph_network: GraphNetwork {
            minimum_indexer_stake: grt_gwei_string_to_f32(
                data.graph_network.minimum_indexer_stake,
            )?,
        },
    })
}

/// Network tracks the GraphcastID's indexer and general Graph network data
#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    pub indexer: Option<Indexer>,
    pub graph_network: GraphNetwork,
}

impl Network {
    /// Fetch indexer staked tokens
    pub fn indexer_stake(&self) -> f32 {
        // TODO: do division by 1e18 for grt unit
        self.indexer
            .as_ref()
            .map(|i| i.staked_tokens)
            .unwrap_or_else(Zero::zero)
    }

    /// Fetch indexer active allocations subgraph deployment IPFS hashes
    pub fn indexer_allocations(&self) -> Vec<String> {
        self.indexer
            .as_ref()
            .map(|i| {
                i.allocations
                    .iter()
                    .map(|a| a.subgraph_deployment.ipfs_hash.clone())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_else(|| [].to_vec())
    }

    pub fn stake_satisfy_requirement(&self) -> bool {
        self.indexer_stake() >= self.graph_network.minimum_indexer_stake
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubgraphDeployment {
    pub ipfs_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub subgraph_deployment: SubgraphDeployment,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Indexer {
    staked_tokens: f32,
    allocations: Vec<Allocation>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphNetwork {
    pub minimum_indexer_stake: f32,
}

#[cfg(test)]
mod tests {
    use num_traits::One;

    use super::*;

    fn dummy_allocations() -> Vec<Allocation> {
        [Allocation {
            subgraph_deployment: SubgraphDeployment {
                ipfs_hash: "Qmdsp5yyFzMVUdSv5N9KndTisjXHrGDEXNaBxjyCTvDfPs".to_string(),
            },
        }]
        .to_vec()
    }

    #[tokio::test]
    async fn stake_minimum_requirement_pass() {
        let network = Network {
            indexer: Some(Indexer {
                staked_tokens: 1_f32,
                allocations: dummy_allocations(),
            }),
            graph_network: GraphNetwork {
                minimum_indexer_stake: Zero::zero(),
            },
        };
        assert_eq!(network.indexer_allocations().len(), 1);
        assert_eq!(network.indexer_stake(), 1.0);
        assert!(network.stake_satisfy_requirement());
    }

    #[tokio::test]
    async fn stake_minimum_requirement_fail() {
        let network = Network {
            indexer: Some(Indexer {
                staked_tokens: Zero::zero(),
                allocations: dummy_allocations(),
            }),
            graph_network: GraphNetwork {
                minimum_indexer_stake: One::one(),
            },
        };
        assert!(!network.stake_satisfy_requirement());
    }

    #[tokio::test]
    async fn stake_minimum_requirement_none() {
        let network = Network {
            indexer: None,
            graph_network: GraphNetwork {
                minimum_indexer_stake: One::one(),
            },
        };

        assert_eq!(network.indexer_allocations().len(), 0);
        assert!(network.indexer_stake().is_zero());
        assert!(network.indexer.is_none());
        assert!(!network.stake_satisfy_requirement());
    }
}
