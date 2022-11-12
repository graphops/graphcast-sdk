#![allow(clippy::all, warnings)]
pub struct GraphAccount;
pub mod graph_account {
    #![allow(dead_code)]
    use std::result::Result;
    pub const OPERATION_NAME: &str = "GraphAccount";
    pub const QUERY : & str = "query GraphAccount($address: String!) {\n  graphAccount(id: $address) {\n    gossipOperatorOf {\n      id\n    }\n  }\n}\n" ;
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
        #[serde(rename = "graphAccount")]
        pub graph_account: Option<GraphAccountGraphAccount>,
    }
    #[derive(Deserialize)]
    pub struct GraphAccountGraphAccount {
        #[serde(rename = "gossipOperatorOf")]
        pub gossip_operator_of: Option<GraphAccountGraphAccountGossipOperatorOf>,
    }
    #[derive(Deserialize)]
    pub struct GraphAccountGraphAccountGossipOperatorOf {
        pub id: String,
    }
}
impl graphql_client::GraphQLQuery for GraphAccount {
    type Variables = graph_account::Variables;
    type ResponseData = graph_account::ResponseData;
    fn build_query(variables: Self::Variables) -> ::graphql_client::QueryBody<Self::Variables> {
        graphql_client::QueryBody {
            variables,
            query: graph_account::QUERY,
            operation_name: graph_account::OPERATION_NAME,
        }
    }
}
