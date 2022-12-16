use std::sync::{Arc, Mutex};

use crate::{
    utils::{LocalAttestationsMap, RemoteAttestationsMap},
    Attestation,
};
use anyhow::anyhow;
use colored::Colorize;

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
                        let remote_attestations = remote_blocks.get(&attestation_block).unwrap();

                        let remote_attestations_clone: Vec<Attestation> = Vec::new();
                        let mut remote_attestations_clone =
                            [remote_attestations_clone, remote_attestations.to_vec()].concat();

                        // Sort remote
                        remote_attestations_clone
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
            Some(vec!["i-am-groot1".to_string()]),
        );

        let attestation2 = Attestation::new(
            "awesome-npoi".to_string(),
            BigUint::default(),
            Some(vec!["i-am-groot2".to_string()]),
        );

        let attestation3 = Attestation::new(
            "awesome-npoi".to_string(),
            BigUint::one(),
            Some(vec!["i-am-groot3".to_string()]),
        );

        let mut attestations = vec![attestation1, attestation2, attestation3];

        attestations.sort_by(|a, b| a.stake_weight.partial_cmp(&b.stake_weight).unwrap());

        assert_eq!(attestations.last().unwrap().stake_weight, BigUint::one());
        assert_eq!(
            attestations
                .last()
                .unwrap()
                .senders
                .as_ref()
                .unwrap()
                .first()
                .unwrap(),
            &"i-am-groot3".to_string()
        );
    }
}
