use std::str::FromStr;

use chrono::Utc;
use ethers::types::RecoveryMessage;
use ethers_contract::EthAbiType;
use ethers_core::types::{transaction::eip712::Eip712, Signature};
use ethers_derive_eip712::*;
use num_bigint::BigUint;
use prost::Message;
use serde::{Deserialize, Serialize};

use crate::{
    client_network::{query_indexer_stake, query_stake_minimum_requirement},
    client_registry::query_registry_indexer,
    constants, message_typing,
};
use anyhow::anyhow;

#[derive(Debug, Eip712, EthAbiType, Serialize, Deserialize)]
#[eip712(
    name = "RadioPaylod",
    version = "1",
    chain_id = 1,
    verifying_contract = "0x0000000000000000000000000000000000000000"
)]
pub struct RadioPayload {
    pub ipfs_hash: String,
    pub npoi: String,
}

impl Clone for RadioPayload {
    fn clone(&self) -> Self {
        Self {
            ipfs_hash: self.ipfs_hash.clone(),
            npoi: self.npoi.clone(),
        }
    }
}

unsafe fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
    ::std::slice::from_raw_parts((p as *const T) as *const u8, ::std::mem::size_of::<T>())
}

impl From<RadioPayloadMessage> for RecoveryMessage {
    fn from(m: RadioPayloadMessage) -> RecoveryMessage {
        RecoveryMessage::Data(unsafe { any_as_u8_slice(&m).to_vec() })
    }
}

#[derive(Eip712, EthAbiType, Clone, Message, Serialize, Deserialize)]
#[eip712(
    name = "GraphcastMessage",
    version = "1",
    chain_id = 1,
    verifying_contract = "0x0000000000000000000000000000000000000000"
)]
pub struct GraphcastMessage {
    #[prost(string, tag = "1")]
    pub subgraph_hash: String,
    #[prost(string, tag = "2")]
    pub npoi: String,
    #[prost(int64, tag = "3")]
    pub nonce: i64,
    #[prost(uint64, tag = "4")]
    pub block_number: u64,
    #[prost(string, tag = "5")]
    pub block_hash: String,
    #[prost(string, tag = "6")]
    pub signature: String,
}

impl GraphcastMessage {
    pub fn new(
        subgraph_hash: String,
        npoi: String,
        nonce: i64,
        block_number: i64,
        block_hash: String,
        signature: String,
    ) -> Self {
        GraphcastMessage {
            subgraph_hash,
            npoi,
            nonce,
            block_number: block_number.try_into().unwrap(),
            block_hash,
            signature,
        }
    }

    // Check message from valid sender: resolve indexer address and self stake
    pub async fn valid_sender(&self) -> Result<&GraphcastMessage, anyhow::Error> {
        let radio_payload =
            message_typing::RadioPayloadMessage::new(self.subgraph_hash.clone(), self.npoi.clone());
        let encoded_message = radio_payload.encode_eip712()?;
        let address = format!(
            "{:#x}",
            Signature::from_str(&self.signature)?.recover(encoded_message)?
        );
        println!("Recovered address from incoming message: {}", address);

        let indexer_address = query_registry_indexer(
            constants::REGISTRY_SUBGRAPH.to_string(),
            address.to_string(),
        )
        .await?;
        let min_req: BigUint =
            query_stake_minimum_requirement(constants::NETWORK_SUBGRAPH.to_string()).await?;
        let sender_stake: BigUint = query_indexer_stake(
            constants::NETWORK_SUBGRAPH.to_string(),
            indexer_address.clone(),
        )
        .await?;
        if sender_stake >= min_req {
            println!(
                "Valid Indexer:  {} : stake {}",
                indexer_address, sender_stake
            );
            Ok(self)
        } else {
            Err(anyhow!(
                "Sender stake is less than the minimum requirement, drop message"
            ))
        }
    }

    // Check timestamp: prevent past message replay
    pub fn valid_time(&self) -> Result<&GraphcastMessage, anyhow::Error> {
        //Can store for measuring overall gossip message latency
        let message_age = Utc::now().timestamp() - self.nonce;
        // 0 allow instant atomic messaging, use 1 to exclude them
        if (0..constants::MSG_REPLAY_LIMIT).contains(&message_age) {
            Ok(self)
        } else {
            Err(anyhow!(
                "Message timestamp {} outside acceptable range {}, drop message",
                message_age,
                constants::MSG_REPLAY_LIMIT
            ))
        }
    }

    // Check timestamp: prevent messages with incorrect provider
    pub fn valid_hash(&self, block_hash: String) -> Result<&GraphcastMessage, anyhow::Error> {
        if self.block_hash == block_hash {
            Ok(self)
        } else {
            Err(anyhow!(
                "Message hash ({}) differ from trusted provider response ({}), drop message",
                self.block_hash,
                block_hash
            ))
        }
    }

    //TODO: FIND A GOOD SOLUTION FOR KEEPING LOCAL NONCES
    //The function signature is required to only take Signal  (FnMut(Signal) + Send + Sync + 'static>(f: F))
    //How to include a nonce map
    // match local_nonces.get(&radio_payload.subgraph_hash.clone()) {
    //     Some(&channel_map) => {
    //         for (sender, nonce) in channel_map.iter_mut() {
    //             println!("Calling {}: {}", sender, nonce);
    //         }
    //         match channel_map.get(&address.to_string()) {
    //             Some(&nonce) if *nonce <= graphcast_message.nonce.clone() => {
    //                 println!("---------------------\nGood nonce good stuff");
    //                 // Update nonce in the map *my_map.get_mut("a").unwrap() += 10;
    //                 // *nonce = graphcast_message.nonce.clone()
    //                 // **channel_map.get_mut(&address.to_string()).unwrap() += graphcast_message.nonce.clone();
    //             },
    //             Some(_) => return Err(anyhow!("---------------------: Message nonce less than local cached nonce, drop message")),
    //             _ => {
    //                 return Err(anyhow!("---------------------: Error parsing local nonce check, drop message"))
    //             }
    //         };
    //     },
    //     None => return Err(anyhow!("Initialize channel map, drop first message")),
    // };
}

#[derive(Eip712, EthAbiType, Clone, Message, Serialize, Deserialize)]
#[eip712(
    name = "radio payload",
    version = "1",
    chain_id = 1,
    verifying_contract = "0x0000000000000000000000000000000000000000"
)]
pub struct RadioPayloadMessage {
    #[prost(string, tag = "1")]
    pub identifier: String,
    #[prost(string, tag = "2")]
    pub content: String,
}

impl RadioPayloadMessage {
    pub fn new(identifier: String, content: String) -> Self {
        RadioPayloadMessage {
            identifier,
            content,
        }
    }
}

// #[derive(Debug, Clone, Serialize, Deserialize)]
// struct NonceMap {
//     topic_sender_nonce_map: HashMap<String, HashMap<String, i32>>,
//   }
