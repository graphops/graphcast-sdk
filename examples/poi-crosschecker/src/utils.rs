use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use graphcast::gossip_agent::GossipAgent;
use num_bigint::BigUint;
use once_cell::sync::OnceCell;

use crate::attestations::Attestation;

pub type RemoteAttestationsMap = HashMap<String, HashMap<u64, Vec<Attestation>>>;
pub type LocalAttestationsMap = HashMap<String, HashMap<u64, Attestation>>;

pub static GOSSIP_AGENT: OnceCell<GossipAgent> = OnceCell::new();
pub static REMOTE_ATTESTATIONS: OnceCell<Arc<Mutex<RemoteAttestationsMap>>> = OnceCell::new();
pub static LOCAL_ATTESTATIONS: OnceCell<Arc<Mutex<LocalAttestationsMap>>> = OnceCell::new();

pub fn update_blocks(
    block_number: u64,
    blocks: &HashMap<u64, Vec<Attestation>>,
    npoi: String,
    stake: BigUint,
    address: String,
) -> HashMap<u64, Vec<Attestation>> {
    let mut blocks_clone: HashMap<u64, Vec<Attestation>> = HashMap::new();
    blocks_clone.extend(blocks.clone());
    blocks_clone.insert(
        block_number,
        vec![Attestation::new(npoi, stake, vec![address])],
    );
    blocks_clone
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_global_maps() {
        _ = REMOTE_ATTESTATIONS.set(Arc::new(Mutex::new(HashMap::new())));
        let mut remote_attestations = REMOTE_ATTESTATIONS.get().unwrap().lock().unwrap();
        let mut blocks: HashMap<u64, Vec<Attestation>> = HashMap::new();
        blocks.insert(
            42,
            vec![Attestation::new(
                "awesome-npoi".to_string(),
                BigUint::default(),
                Vec::new(),
            )],
        );
        remote_attestations.insert("my-subgraph-hash".to_string(), blocks);

        let blocks = remote_attestations.get("my-subgraph-hash").unwrap();
        assert_eq!(
            blocks.get(&42).unwrap().first().unwrap().npoi,
            "awesome-npoi".to_string()
        );

        _ = LOCAL_ATTESTATIONS.set(Arc::new(Mutex::new(HashMap::new())));
        let mut local_attestations = LOCAL_ATTESTATIONS.get().unwrap().lock().unwrap();
        let mut blocks: HashMap<u64, Attestation> = HashMap::new();
        blocks.insert(
            42,
            Attestation::new("awesome-npoi".to_string(), BigUint::default(), Vec::new()),
        );
        local_attestations.insert("my-subgraph-hash".to_string(), blocks);

        let blocks = local_attestations.get("my-subgraph-hash").unwrap();
        assert_eq!(blocks.get(&42).unwrap().npoi, "awesome-npoi".to_string());
    }

    #[test]
    fn test_update_blocks() {
        let mut blocks: HashMap<u64, Vec<Attestation>> = HashMap::new();
        blocks.insert(
            42,
            vec![Attestation::new(
                "default".to_string(),
                BigUint::default(),
                Vec::new(),
            )],
        );
        let block_clone = update_blocks(
            42,
            &blocks,
            "awesome-npoi".to_string(),
            BigUint::default(),
            "address".to_string(),
        );

        assert_eq!(
            block_clone.get(&42).unwrap().first().unwrap().npoi,
            "awesome-npoi".to_string()
        );
    }
}
