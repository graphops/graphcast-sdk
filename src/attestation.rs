use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Attestation {
    pub subgraph: String,
    pub block: u64,
    pub npoi: String,
    pub indexer: String,
    pub stake_weight: String,
    pub nonce: i64,
}

impl Attestation {
    pub fn new(
        subgraph: String,
        block: u64,
        npoi: String,
        indexer: String,
        stake_weight: String,
        nonce: i64,
    ) -> Self {
        Attestation {
            subgraph,
            block,
            npoi,
            indexer,
            stake_weight,
            nonce,
        }
    }
}
