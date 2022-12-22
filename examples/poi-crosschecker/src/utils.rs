use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::Mutex as AsyncMutex;

use graphcast_sdk::gossip_agent::{
    message_typing::{get_indexer_stake, GraphcastMessage},
    GossipAgent,
};
use num_bigint::BigUint;
use once_cell::sync::OnceCell;

use crate::attestations::Attestation;

pub type RemoteAttestationsMap = HashMap<String, HashMap<u64, Vec<Attestation>>>;
pub type LocalAttestationsMap = HashMap<String, HashMap<u64, Attestation>>;

/// A global static (singleton) instance of GossipAgent. It is useful to ensure that we have only one GossipAgent
/// per Radio instance, so that we can keep track of state and more easily test our Radio application.
pub static GOSSIP_AGENT: OnceCell<GossipAgent> = OnceCell::new();

/// A global static (singleton) instance of A GraphcastMessage vector.
/// It is used to save incoming messages after they've been validated, in order
/// defer their processing for later, because async code is required for the processing but
/// it is not allowed in the handler itself.
pub static MESSAGES: OnceCell<Arc<Mutex<Vec<GraphcastMessage>>>> = OnceCell::new();

/// Updates the `blocks` HashMap to include the new attestation.
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

/// This function processes the global messages map that we populate when
/// messages are being received. It constructs the remote attestations
/// map and returns it if the processing succeeds.
pub async fn process_messages(
    messages: Arc<Mutex<Vec<GraphcastMessage>>>,
) -> Result<RemoteAttestationsMap, anyhow::Error> {
    let mut remote_attestations: RemoteAttestationsMap = HashMap::new();
    let messages = AsyncMutex::new(messages.lock().unwrap());

    for msg in messages.lock().await.iter() {
        let sender = msg.recover_sender_address()?;
        let sender_stake = get_indexer_stake(sender.clone()).await?;
        let blocks = remote_attestations.get(&msg.identifier.to_string());

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
                                    sender.clone(),
                                    sender_stake.clone(),
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
                                        msg.content.to_string(),
                                        sender_stake.clone(),
                                        sender,
                                    );
                                    remote_attestations
                                        .insert(msg.identifier.to_string(), blocks_clone);
                                }
                            }
                            None => {
                                let blocks_clone = update_blocks(
                                    msg.block_number,
                                    blocks,
                                    msg.content.to_string(),
                                    sender_stake.clone(),
                                    sender,
                                );
                                remote_attestations
                                    .insert(msg.identifier.to_string(), blocks_clone);
                            }
                        }
                    }
                    None => {
                        let blocks_clone = update_blocks(
                            msg.block_number,
                            blocks,
                            msg.content.to_string(),
                            sender_stake.clone(),
                            sender,
                        );
                        remote_attestations.insert(msg.identifier.to_string(), blocks_clone);
                    }
                }
            }
            None => {
                let blocks_clone = update_blocks(
                    msg.block_number,
                    &HashMap::new(),
                    msg.content.to_string(),
                    sender_stake.clone(),
                    sender,
                );
                remote_attestations.insert(msg.identifier.to_string(), blocks_clone);
            }
        }
    }

    Ok(remote_attestations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_global_map() {
        _ = MESSAGES.set(Arc::new(Mutex::new(Vec::new())));
        let mut messages = MESSAGES.get().unwrap().lock().unwrap();

        let hash: String = "QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U".to_string();
        let content: String =
            "0xa6008cea5905b8b7811a68132feea7959b623188e2d6ee3c87ead7ae56dd0eae".to_string();
        let nonce: i64 = 123321;
        let block_number: i64 = 0;
        let block_hash: String = "0xblahh".to_string();
        let sig: String = "4be6a6b7f27c4086f22e8be364cbdaeddc19c1992a42b08cbe506196b0aafb0a68c8c48a730b0e3155f4388d7cc84a24b193d091c4a6a4e8cd6f1b305870fae61b".to_string();
        let msg = GraphcastMessage::new(
            hash,
            content.clone(),
            nonce,
            block_number,
            block_hash.clone(),
            sig,
        );

        assert!(messages.is_empty());

        messages.push(msg);
        assert_eq!(
            messages.first().unwrap().identifier,
            "QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U".to_string()
        );
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

    #[tokio::test]
    async fn test_process_messages() {
        let hash: String = "QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U".to_string();
        let content: String =
            "0xa6008cea5905b8b7811a68132feea7959b623188e2d6ee3c87ead7ae56dd0eae".to_string();
        let nonce: i64 = 123321;
        let block_number: i64 = 0;
        let block_hash: String = "0xblahh".to_string();
        let sig: String = "4be6a6b7f27c4086f22e8be364cbdaeddc19c1992a42b08cbe506196b0aafb0a68c8c48a730b0e3155f4388d7cc84a24b193d091c4a6a4e8cd6f1b305870fae61b".to_string();
        let msg1 = GraphcastMessage::new(
            hash,
            content.clone(),
            nonce,
            block_number,
            block_hash.clone(),
            sig,
        );

        let hash: String = "QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U".to_string();
        let content: String =
            "0xa6008cea5905b8b7811a68132feea7959b623188e2d6ee3c87ead7ae56dd0eae".to_string();
        let nonce: i64 = 123321;
        let block_number: i64 = 0;
        let block_hash: String = "0xblahh".to_string();
        let sig: String = "4be6a6b7f27c4086f22e8be364cbdaeddc19c1992a42b08cbe506196b0aafb0a68c8c48a730b0e3155f4388d7cc84a24b193d091c4a6a4e8cd6f1b305870fae61b".to_string();
        let msg2 = GraphcastMessage::new(
            hash,
            content.clone(),
            nonce,
            block_number,
            block_hash.clone(),
            sig,
        );

        let parsed = process_messages(Arc::new(Mutex::new(vec![msg1, msg2]))).await;
        assert!(parsed.is_ok());
    }

    #[test]
    fn test_delete_messages() {
        _ = MESSAGES.set(Arc::new(Mutex::new(Vec::new())));

        let mut messages = MESSAGES.get().unwrap().lock().unwrap();

        let hash: String = "QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U".to_string();
        let content: String =
            "0xa6008cea5905b8b7811a68132feea7959b623188e2d6ee3c87ead7ae56dd0eae".to_string();
        let nonce: i64 = 123321;
        let block_number: i64 = 0;
        let block_hash: String = "0xblahh".to_string();
        let sig: String = "4be6a6b7f27c4086f22e8be364cbdaeddc19c1992a42b08cbe506196b0aafb0a68c8c48a730b0e3155f4388d7cc84a24b193d091c4a6a4e8cd6f1b305870fae61b".to_string();
        let msg = GraphcastMessage::new(
            hash,
            content.clone(),
            nonce,
            block_number,
            block_hash.clone(),
            sig,
        );

        messages.push(msg);
        assert!(!messages.is_empty());

        messages.clear();
        assert!(messages.is_empty());
    }
}
