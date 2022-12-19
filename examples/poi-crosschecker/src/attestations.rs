use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::utils::{
    update_blocks, LocalAttestationsMap, RemoteAttestationsMap, LOCAL_ATTESTATIONS,
    REMOTE_ATTESTATIONS,
};
use anyhow::anyhow;
use colored::Colorize;
use graphcast::gossip_agent::message_typing::MessageWithCtx;
use num_bigint::BigUint;

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

pub fn save_local_attestation(attestation: Attestation, ipfs_hash: String, block_number: u64) {
    let mut local_attestations = LOCAL_ATTESTATIONS.get().unwrap().lock().unwrap();
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

pub fn attestation_handler() -> impl Fn(Result<MessageWithCtx, anyhow::Error>) {
    |msg_with_ctx: Result<MessageWithCtx, anyhow::Error>| match msg_with_ctx {
        Ok(msg_with_ctx) => {
            let msg = msg_with_ctx.message;

            let mut remote_attestations = REMOTE_ATTESTATIONS.get().unwrap().lock().unwrap();
            let blocks = remote_attestations.get(&msg.identifier);

            match blocks {
                Some(blocks) => {
                    // Already has attestations for that block
                    let attestations = blocks.get(&msg.block_number);
                    match attestations {
                        Some(attestations) => {
                            let attestations_clone: Vec<Attestation> = Vec::new();
                            let attestations_clone =
                                [attestations_clone, attestations.to_vec()].concat();

                            let existing_attestation =
                                attestations_clone.iter().find(|a| a.npoi == msg.content);

                            match existing_attestation {
                                Some(existing_attestation) => {
                                    let updated_attestation = Attestation::update(
                                        existing_attestation,
                                        msg_with_ctx.sender.clone(),
                                        msg_with_ctx.sender_stake.clone(),
                                    );
                                    if let Err(err) = updated_attestation {
                                        println!("{}", err);
                                    } else {
                                        attestations_clone
                                            .iter()
                                            .find(|a| a.npoi == existing_attestation.npoi)
                                            .map(|_| &updated_attestation);

                                        // Update map
                                        let blocks_clone = update_blocks(
                                            msg.block_number,
                                            blocks,
                                            msg.content,
                                            msg_with_ctx.sender_stake.clone(),
                                            msg_with_ctx.sender,
                                        );
                                        remote_attestations.insert(msg.identifier, blocks_clone);
                                    }
                                }
                                None => {
                                    let blocks_clone = update_blocks(
                                        msg.block_number,
                                        blocks,
                                        msg.content,
                                        msg_with_ctx.sender_stake.clone(),
                                        msg_with_ctx.sender,
                                    );
                                    remote_attestations.insert(msg.identifier, blocks_clone);
                                }
                            }
                        }
                        None => {
                            let blocks_clone = update_blocks(
                                msg.block_number,
                                blocks,
                                msg.content,
                                msg_with_ctx.sender_stake.clone(),
                                msg_with_ctx.sender,
                            );
                            remote_attestations.insert(msg.identifier, blocks_clone);
                        }
                    }
                }
                None => {
                    let blocks_clone = update_blocks(
                        msg.block_number,
                        &HashMap::new(),
                        msg.content,
                        msg_with_ctx.sender_stake.clone(),
                        msg_with_ctx.sender,
                    );
                    remote_attestations.insert(msg.identifier, blocks_clone);
                }
            }
        }
        Err(err) => {
            println!("{}", err);
        }
    }
}

pub fn compare_attestations(
    attestation_block: u64,
    remote: Arc<Mutex<RemoteAttestationsMap>>,
    local: Arc<Mutex<LocalAttestationsMap>>,
) -> Result<String, anyhow::Error> {
    let remote = remote.lock().unwrap();
    let local = local.lock().unwrap();

    // Iterate & compare
    if let Some((ipfs_hash, blocks)) = local.iter().next() {
        let attestations = blocks.get(&attestation_block);
        match attestations {
            Some(local_attestation) => {
                let remote_blocks = remote.get(ipfs_hash);
                match remote_blocks {
                    Some(remote_blocks) => {
                        // Unwrapping because we're sure that if there's an entry for the subgraph there will also be at least one attestation
                        let mut remote_attestations = remote_blocks.get(&attestation_block).unwrap().clone();

                        // Sort remote
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
                            // Take some action - send alert, close allocations, etc
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
        _ = REMOTE_ATTESTATIONS.set(Arc::new(Mutex::new(HashMap::new())));
        _ = LOCAL_ATTESTATIONS.set(Arc::new(Mutex::new(HashMap::new())));

        let res = compare_attestations(
            42,
            Arc::clone(REMOTE_ATTESTATIONS.get().unwrap()),
            Arc::clone(LOCAL_ATTESTATIONS.get().unwrap()),
        );

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

        _ = REMOTE_ATTESTATIONS.set(Arc::new(Mutex::new(remote_attestations)));
        _ = LOCAL_ATTESTATIONS.set(Arc::new(Mutex::new(local_attestations)));

        let res = compare_attestations(
            42,
            Arc::clone(REMOTE_ATTESTATIONS.get().unwrap()),
            Arc::clone(LOCAL_ATTESTATIONS.get().unwrap()),
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

        _ = REMOTE_ATTESTATIONS.set(Arc::new(Mutex::new(remote_attestations)));
        _ = LOCAL_ATTESTATIONS.set(Arc::new(Mutex::new(local_attestations)));

        let res = compare_attestations(
            42,
            Arc::clone(REMOTE_ATTESTATIONS.get().unwrap()),
            Arc::clone(LOCAL_ATTESTATIONS.get().unwrap()),
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

        _ = REMOTE_ATTESTATIONS.set(Arc::new(Mutex::new(remote_attestations)));
        _ = LOCAL_ATTESTATIONS.set(Arc::new(Mutex::new(local_attestations)));

        let res = compare_attestations(
            42,
            Arc::clone(REMOTE_ATTESTATIONS.get().unwrap()),
            Arc::clone(LOCAL_ATTESTATIONS.get().unwrap()),
        );

        assert!(res.is_ok());
        assert_eq!(
            res.unwrap(),
            "POIs match for subgraph my-awesome-hash on block 42!".to_string()
        );
    }
}
