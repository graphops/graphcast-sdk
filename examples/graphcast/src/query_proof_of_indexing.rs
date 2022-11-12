#![allow(clippy::all, warnings)]
pub struct ProofOfIndexing;
pub mod proof_of_indexing {
    #![allow(dead_code)]
    use std::result::Result;
    pub const OPERATION_NAME: &str = "ProofOfIndexing";
    pub const QUERY : & str = "query ProofOfIndexing($subgraph: String!, $blockNumber: Int!, $blockHash: String!, $indexer: String) {\n    proofOfIndexing(\n      subgraph: $subgraph\n      blockNumber: $blockNumber\n      blockHash: $blockHash\n      indexer: $indexer\n    )\n}\n" ;
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
        pub subgraph: String,
        #[serde(rename = "blockNumber")]
        pub block_number: Int,
        #[serde(rename = "blockHash")]
        pub block_hash: String,
        pub indexer: Option<String>,
    }
    impl Variables {}
    #[derive(Deserialize)]
    pub struct ResponseData {
        #[serde(rename = "proofOfIndexing")]
        pub proof_of_indexing: Option<String>,
    }
}
impl graphql_client::GraphQLQuery for ProofOfIndexing {
    type Variables = proof_of_indexing::Variables;
    type ResponseData = proof_of_indexing::ResponseData;
    fn build_query(variables: Self::Variables) -> ::graphql_client::QueryBody<Self::Variables> {
        graphql_client::QueryBody {
            variables,
            query: proof_of_indexing::QUERY,
            operation_name: proof_of_indexing::OPERATION_NAME,
        }
    }
}
