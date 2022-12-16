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
    npoi: String,
    stake: BigUint,
    address: String,
) -> HashMap<u64, Vec<Attestation>> {
    let mut blocks_clone: HashMap<u64, Vec<Attestation>> = HashMap::new();
    blocks_clone.extend(blocks.clone());
    blocks_clone.insert(
        block_number,
        vec![Attestation::new(npoi, stake, Some(vec![address]))],
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
    use crate::compare_attestations::compare_attestations;

    use super::*;
    use num_traits::identities::One;

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

    #[test]
    fn test_clone_blocks() {
        let mut blocks: HashMap<u64, Vec<Attestation>> = HashMap::new();
        blocks.insert(
            42,
            vec![Attestation::new(
                "default".to_string(),
                BigUint::default(),
                None,
            )],
        );
        let block_clone = clone_blocks(
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

    #[test]
    fn test_attestation_update() {
        let attestation = Attestation::new(
            "awesome-npoi".to_string(),
            BigUint::default(),
            Some(vec!["i-am-groot".to_string()]),
        );

        let updated_attestation =
            Attestation::update(&attestation, "soggip".to_string(), BigUint::one());

        assert_eq!(updated_attestation.stake_weight, BigUint::one());
    }

    #[test]
    fn test_compare_attestations_fail() {
        let _ = REMOTE_ATTESTATIONS.set(Arc::new(Mutex::new(HashMap::new())));
        let _ = LOCAL_ATTESTATIONS.set(Arc::new(Mutex::new(HashMap::new())));

        let res = compare_attestations(
            42,
            Arc::clone(REMOTE_ATTESTATIONS.get().unwrap()),
            Arc::clone(LOCAL_ATTESTATIONS.get().unwrap()),
        );

        assert!(res.is_err());
    }

    #[test]
    fn test_compare_attestations_success() {
        let mut remote_blocks: HashMap<u64, Vec<Attestation>> = HashMap::new();
        let mut local_blocks: HashMap<u64, Attestation> = HashMap::new();

        remote_blocks.insert(
            42,
            vec![Attestation::new(
                "awesome-npoi".to_string(),
                BigUint::default(),
                Some(vec!["i-am-groot".to_string()]),
            )],
        );

        local_blocks.insert(
            42,
            Attestation::new("awesome-npoi".to_string(), BigUint::default(), None),
        );

        let mut remote_attestations: HashMap<String, HashMap<u64, Vec<Attestation>>> = HashMap::new();
        let mut local_attestations: HashMap<String, HashMap<u64, Attestation>> = HashMap::new();

        remote_attestations.insert("my-awesome-hash".to_string(), remote_blocks);
        local_attestations.insert("my-awesome-hash".to_string(), local_blocks);

        let _ = REMOTE_ATTESTATIONS.set(Arc::new(Mutex::new(remote_attestations)));
        let _ = LOCAL_ATTESTATIONS.set(Arc::new(Mutex::new(local_attestations)));

        let res = compare_attestations(
            42,
            Arc::clone(&REMOTE_ATTESTATIONS.get().unwrap()),
            Arc::clone(&LOCAL_ATTESTATIONS.get().unwrap()),
        );

        assert!(res.is_ok());
    }
}
