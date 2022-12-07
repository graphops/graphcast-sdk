#![allow(clippy::all, warnings)]
pub struct Indexer;
pub mod indexer {
    #![allow(dead_code)]
    use std::result::Result;
    pub const OPERATION_NAME: &str = "Indexer";
    pub const QUERY : & str = "query Indexer($address: String!) {\n  indexer(id: $address) {\n    stakedTokens\n    allocations{\n      subgraphDeployment{\n        ipfsHash\n      }\n    }\n  }\n  graphNetwork(id: 1) {\n    minimumIndexerStake\n  }\n}\n" ;
    use super::*;
    use serde::{Deserialize, Serialize};
    #[allow(dead_code)]
    type Boolean = bool;
    #[allow(dead_code)]
    type Float = f64;
    #[allow(dead_code)]
    type Int = i64;
    #[allow(dead_code)]
    type ID = String;
    #[derive(Serialize)]
    pub struct Variables {
        pub address: String,
    }
    impl Variables {}
    #[derive(Deserialize)]
    pub struct ResponseData {
        pub indexer: Option<IndexerIndexer>,
        #[serde(rename = "graphNetwork")]
        pub graph_network: IndexerGraphNetwork,
    }
    #[derive(Deserialize)]
    pub struct IndexerIndexer {
        #[serde(rename = "stakedTokens")]
        pub staked_tokens: Option<Int>,
        pub allocations: Option<Vec<IndexerIndexerAllocations>>,
    }
    #[derive(Deserialize)]
    pub struct IndexerIndexerAllocations {
        #[serde(rename = "subgraphDeployment")]
        pub subgraph_deployment: IndexerIndexerAllocationsSubgraphDeployment,
    }
    #[derive(Deserialize)]
    pub struct IndexerIndexerAllocationsSubgraphDeployment {
        #[serde(rename = "ipfsHash")]
        pub ipfs_hash: String,
    }
    #[derive(Deserialize)]
    pub struct IndexerGraphNetwork {
        #[serde(rename = "minimumIndexerStake")]
        pub minimum_indexer_stake: Int,
    }
}
impl graphql_client::GraphQLQuery for Indexer {
    type Variables = indexer::Variables;
    type ResponseData = indexer::ResponseData;
    fn build_query(variables: Self::Variables) -> ::graphql_client::QueryBody<Self::Variables> {
        graphql_client::QueryBody {
            variables,
            query: indexer::QUERY,
            operation_name: indexer::OPERATION_NAME,
        }
    }
}
