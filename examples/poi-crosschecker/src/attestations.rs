use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::utils::{LocalAttestationsMap, RemoteAttestationsMap, MESSAGES};
use anyhow::anyhow;
use colored::Colorize;
use graphcast_sdk::gossip_agent::message_typing::GraphcastMessage;
use num_bigint::BigUint;

/// A wrapper around an attested NPOI, tracks Indexers that have sent it plus their accumulated stake
#[derive(Clone, Debug)]
pub struct Attestation {
    pub npoi: String,
    pub stake_weight: BigUint,
    pub senders: Vec<String>,
}

impl Attestation {
    pub fn new(npoi: String, stake_weight: BigUint, senders: Vec<String>) -> Self {
        Attestation {
            npoi,
            stake_weight,
            senders,
        }
    }

    /// Used whenever we receive a new attestation for an NPOI that already exists in the store
    pub fn update(base: &Self, address: String, stake: BigUint) -> Result<Self, anyhow::Error> {
        if base.senders.contains(&address) {
            Err(anyhow!(
                "{}",
                "There is already an attestation from this address. Skipping..."
                    .to_string()
                    .yellow()
            ))
        } else {
            let senders = [base.senders.clone(), vec![address]].concat();
            Ok(Self::new(
                base.npoi.clone(),
                base.stake_weight.clone() + stake,
                senders,
            ))
        }
    }
}

/// Saves NPOIs that we've generated locally, in order to compare them with remote ones later
pub fn save_local_attestation(
    local_attestations: &mut LocalAttestationsMap,
    attestation: Attestation,
    ipfs_hash: String,
    block_number: u64,
) {
    let blocks = local_attestations.get(&ipfs_hash);

    match blocks {
        Some(blocks) => {
            let mut blocks_clone: HashMap<u64, Attestation> = HashMap::new();
            blocks_clone.extend(blocks.clone());
            blocks_clone.insert(block_number, attestation);
            local_attestations.insert(ipfs_hash, blocks_clone);
        }
        None => {
            let mut blocks_clone: HashMap<u64, Attestation> = HashMap::new();
            blocks_clone.insert(block_number, attestation);
            local_attestations.insert(ipfs_hash, blocks_clone);
        }
    }
}

/// Custom callback for handling the validated GraphcastMessage, in this case we only save the messages to a local store
/// to process them at a later time. This is required because for the processing we use async operations which are not allowed
/// in the handler.
pub fn attestation_handler() -> impl Fn(Result<GraphcastMessage, anyhow::Error>) {
    |msg: Result<GraphcastMessage, anyhow::Error>| match msg {
        Ok(msg) => {
            MESSAGES.get().unwrap().lock().unwrap().push(msg);
        }
        Err(err) => {
            println!("{}", err);
        }
    }
}

/// Compares local attestations against remote ones using the attestation stores we populated while processing saved GraphcastMessage messages.
/// It takes our attestation (NPOI) for a given subgraph on a given block and compares it to the top-attested one from the remote attestations.
/// The top remote attestation is found by grouping attestations together and increasing their total stake-weight every time we see a new message
/// with the same NPOI from an Indexer (NOTE: one Indexer can only send 1 attestation per subgraph per block). The attestations are then sorted
/// and we take the one with the highest total stake-weight.
pub fn compare_attestations(
    attestation_block: u64,
    remote: RemoteAttestationsMap,
    local: Arc<Mutex<LocalAttestationsMap>>,
) -> Result<String, anyhow::Error> {
    let local = local.lock().unwrap();

    // Iterate & compare
    if let Some((ipfs_hash, blocks)) = local.iter().next() {
        let attestations = blocks.get(&attestation_block);
        match attestations {
            Some(local_attestation) => {
                let remote_blocks = remote.get(ipfs_hash);
                match remote_blocks {
                    Some(remote_blocks) => {
                        let remote_attestations = remote_blocks.get(&attestation_block);

                        match remote_attestations {
                            Some(remote_attestations) => {
                                let mut remote_attestations = remote_attestations.clone();

                        remote_attestations
                        .sort_by(|a, b| a.stake_weight.partial_cmp(&b.stake_weight).unwrap());

                    let most_attested_npoi = &remote_attestations.last().unwrap().npoi;
                    if most_attested_npoi == &local_attestation.npoi {
                        return Ok(format!(
                            "POIs match for subgraph {} on block {}!",
                            ipfs_hash, attestation_block
                        ));
                    } else {
                        return Err(anyhow!(format!(
                            "POIs don't match for subgraph {} on block {}!",
                            ipfs_hash, attestation_block
                        )
                        .red()
                        .bold()));
                    }
                            },
                            None => {
                                return Err(anyhow!(format!(
                                    "No record for subgraph {} on block {} found in remote attestations",
                                    ipfs_hash, attestation_block
                                )
                                .yellow()
                               ));
                            }
                        }
                    }
                    None => {
                        return Err(anyhow!(format!("No attestations for subgraph {} on block {} found in remote attestations store. Continuing...", ipfs_hash, attestation_block, ).yellow()))
                    }
                }
            }
            None => {
                return Err(anyhow!(format!("No attestation for subgraph {} on block {} found in local attestations store. Continuing...", ipfs_hash, attestation_block, ).yellow()))
            }
        }
    }

    Err(anyhow!(format!(
        "The comparison did not execute successfully for on block {}. Continuing...",
        attestation_block,
    )
    .yellow()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigUint;
    use num_traits::identities::One;

    #[test]
    fn test_attestation_sorting() {
        let attestation1 = Attestation::new(
            "awesome-npoi".to_string(),
            BigUint::default(),
            vec!["i-am-groot1".to_string()],
        );

        let attestation2 = Attestation::new(
            "awesome-npoi".to_string(),
            BigUint::default(),
            vec!["i-am-groot2".to_string()],
        );

        let attestation3 = Attestation::new(
            "awesome-npoi".to_string(),
            BigUint::one(),
            vec!["i-am-groot3".to_string()],
        );

        let mut attestations = vec![attestation1, attestation2, attestation3];

        attestations.sort_by(|a, b| a.stake_weight.partial_cmp(&b.stake_weight).unwrap());

        assert_eq!(attestations.last().unwrap().stake_weight, BigUint::one());
        assert_eq!(
            attestations.last().unwrap().senders.first().unwrap(),
            &"i-am-groot3".to_string()
        );
    }

    #[test]
    fn test_attestation_update_success() {
        let attestation = Attestation::new(
            "awesome-npoi".to_string(),
            BigUint::default(),
            vec!["i-am-groot".to_string()],
        );

        let updated_attestation =
            Attestation::update(&attestation, "soggip".to_string(), BigUint::one());

        assert!(updated_attestation.is_ok());
        assert_eq!(updated_attestation.unwrap().stake_weight, BigUint::one());
    }

    #[test]
    fn test_attestation_update_fail() {
        let attestation = Attestation::new(
            "awesome-npoi".to_string(),
            BigUint::default(),
            vec!["i-am-groot".to_string()],
        );

        let updated_attestation =
            Attestation::update(&attestation, "i-am-groot".to_string(), BigUint::default());

        assert!(updated_attestation.is_err());
        assert_eq!(
            updated_attestation.unwrap_err().to_string(),
            "There is already an attestation from this address. Skipping...".to_string()
        );
    }

    #[test]
    fn test_compare_attestations_generic_fail() {
        let res = compare_attestations(42, HashMap::new(), Arc::new(Mutex::new(HashMap::new())));

        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().to_string(),
            "The comparison did not execute successfully for on block 42. Continuing..."
                .to_string()
        );
    }

    #[test]
    fn test_compare_attestations_remote_not_found_fail() {
        let mut remote_blocks: HashMap<u64, Vec<Attestation>> = HashMap::new();
        let mut local_blocks: HashMap<u64, Attestation> = HashMap::new();

        remote_blocks.insert(
            42,
            vec![Attestation::new(
                "awesome-npoi".to_string(),
                BigUint::default(),
                vec!["i-am-groot".to_string()],
            )],
        );

        local_blocks.insert(
            42,
            Attestation::new("awesome-npoi".to_string(), BigUint::default(), Vec::new()),
        );

        let mut remote_attestations: HashMap<String, HashMap<u64, Vec<Attestation>>> =
            HashMap::new();
        let mut local_attestations: HashMap<String, HashMap<u64, Attestation>> = HashMap::new();

        remote_attestations.insert("my-awesome-hash".to_string(), remote_blocks);
        local_attestations.insert("different-awesome-hash".to_string(), local_blocks);

        let res = compare_attestations(
            42,
            remote_attestations,
            Arc::new(Mutex::new(local_attestations)),
        );

        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(),"No attestations for subgraph different-awesome-hash on block 42 found in remote attestations store. Continuing...".to_string());
    }

    #[test]
    fn test_compare_attestations_local_not_found_fail() {
        let remote_blocks: HashMap<u64, Vec<Attestation>> = HashMap::new();
        let local_blocks: HashMap<u64, Attestation> = HashMap::new();

        let mut remote_attestations: HashMap<String, HashMap<u64, Vec<Attestation>>> =
            HashMap::new();
        let mut local_attestations: HashMap<String, HashMap<u64, Attestation>> = HashMap::new();

        remote_attestations.insert("my-awesome-hash".to_string(), remote_blocks);
        local_attestations.insert("my-awesome-hash".to_string(), local_blocks);

        let res = compare_attestations(
            42,
            remote_attestations,
            Arc::new(Mutex::new(local_attestations)),
        );

        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(),"No attestation for subgraph my-awesome-hash on block 42 found in local attestations store. Continuing...".to_string());
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
                vec!["i-am-groot".to_string()],
            )],
        );

        local_blocks.insert(
            42,
            Attestation::new("awesome-npoi".to_string(), BigUint::default(), Vec::new()),
        );

        let mut remote_attestations: HashMap<String, HashMap<u64, Vec<Attestation>>> =
            HashMap::new();
        let mut local_attestations: HashMap<String, HashMap<u64, Attestation>> = HashMap::new();

        remote_attestations.insert("my-awesome-hash".to_string(), remote_blocks);
        local_attestations.insert("my-awesome-hash".to_string(), local_blocks);

        let res = compare_attestations(
            42,
            remote_attestations,
            Arc::new(Mutex::new(local_attestations)),
        );

        assert!(res.is_ok());
        assert_eq!(
            res.unwrap(),
            "POIs match for subgraph my-awesome-hash on block 42!".to_string()
        );
    }
}
