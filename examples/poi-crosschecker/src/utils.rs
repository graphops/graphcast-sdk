use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use graphcast::gossip_agent::GossipAgent;
use num_bigint::BigUint;
use once_cell::sync::OnceCell;

#[derive(Clone, Debug)]
pub struct Attestation {
    pub npoi: String,
    pub stake_weight: BigUint,
    pub senders: Option<Vec<String>>,
}

impl Attestation {
    pub fn new(npoi: String, stake_weight: BigUint, senders: Option<Vec<String>>) -> Self {
        Attestation {
            npoi,
            stake_weight,
            senders,
        }
    }

    pub fn update(base: &Self, address: String, stake: BigUint) -> Self {
        let senders = Some([base.senders.as_ref().unwrap().clone(), vec![address]].concat());
        Self::new(
            base.npoi.clone(),
            base.stake_weight.clone() + stake,
            senders,
        )
    }
}

pub fn clone_blocks(
    block_number: u64,
    blocks: &HashMap<u64, Vec<Attestation>>,
    ipfs_hash: String,
    stake: BigUint,
    address: String,
) -> HashMap<u64, Vec<Attestation>> {
    let mut blocks_clone: HashMap<u64, Vec<Attestation>> = HashMap::new();
    blocks_clone.extend(blocks.clone());
    blocks_clone.insert(
        block_number,
        vec![Attestation::new(ipfs_hash, stake, Some(vec![address]))],
    );
    blocks_clone
}

pub type RemoteAttestationsMap = HashMap<String, HashMap<u64, Vec<Attestation>>>;
pub type LocalAttestationsMap = HashMap<String, HashMap<u64, Attestation>>;

pub static GOSSIP_AGENT: OnceCell<GossipAgent> = OnceCell::new();
pub static REMOTE_ATTESTATIONS: OnceCell<Arc<Mutex<RemoteAttestationsMap>>> = OnceCell::new();
pub static LOCAL_ATTESTATIONS: OnceCell<Arc<Mutex<LocalAttestationsMap>>> = OnceCell::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_global_maps() {
        let _ = REMOTE_ATTESTATIONS.set(Arc::new(Mutex::new(HashMap::new())));
        let mut remote_attestations = REMOTE_ATTESTATIONS.get().unwrap().lock().unwrap();
        let mut blocks: HashMap<u64, Vec<Attestation>> = HashMap::new();
        blocks.insert(
            42,
            vec![Attestation::new(
                "awesome-npoi".to_string(),
                BigUint::default(),
                None,
            )],
        );
        remote_attestations.insert("my-subgraph-hash".to_string(), blocks);

        let blocks = remote_attestations.get("my-subgraph-hash").unwrap();
        assert_eq!(
            blocks.get(&42).unwrap().first().unwrap().npoi,
            "awesome-npoi".to_string()
        );

        let _ = LOCAL_ATTESTATIONS.set(Arc::new(Mutex::new(HashMap::new())));
        let mut local_attestations = LOCAL_ATTESTATIONS.get().unwrap().lock().unwrap();
        let mut blocks: HashMap<u64, Attestation> = HashMap::new();
        blocks.insert(
            42,
            Attestation::new("awesome-npoi".to_string(), BigUint::default(), None),
        );
        local_attestations.insert("my-subgraph-hash".to_string(), blocks);

        let blocks = local_attestations.get("my-subgraph-hash").unwrap();
        assert_eq!(blocks.get(&42).unwrap().npoi, "awesome-npoi".to_string());
    }
}
