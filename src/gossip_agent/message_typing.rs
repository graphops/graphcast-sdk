use std::{
    collections::HashMap,
    error::Error,
    str::FromStr,
    sync::{Arc, Mutex},
};

use chrono::Utc;
use colored::Colorize;
use ethers::{
    signers::{Signer, Wallet},
    types::RecoveryMessage,
};
use ethers_contract::EthAbiType;
use ethers_core::{
    k256::ecdsa::SigningKey,
    types::{transaction::eip712::Eip712, Signature},
};
use ethers_derive_eip712::*;
use num_bigint::BigUint;
use prost::Message;
use serde::{Deserialize, Serialize};
use waku::{Running, WakuContentTopic, WakuMessage, WakuNodeHandle, WakuPubSubTopic};

use crate::{
    graphql::{client_network::query_network_subgraph, client_registry::query_registry_indexer},
    NoncesMap,
};
use anyhow::anyhow;

use super::{MSG_REPLAY_LIMIT, NETWORK_SUBGRAPH, REGISTRY_SUBGRAPH};

/// Radio payload that includes an grpah identifier and custom content
/// In future work, allow dynamic buffers
#[derive(Debug, Eip712, EthAbiType, Serialize, Deserialize)]
#[eip712(
    name = "RadioPaylod",
    version = "1",
    chain_id = 1,
    verifying_contract = "0x0000000000000000000000000000000000000000"
)]
pub struct RadioPayload {
    /// Graph identifier for the entity the radio is communicating about
    pub identifier: String,
    /// content to share about the identified entity
    pub content: String,
}

impl Clone for RadioPayload {
    fn clone(&self) -> Self {
        Self {
            identifier: self.identifier.clone(),
            content: self.content.clone(),
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
/// Prepare sender:nonce to update
fn prepare_nonces(
    nonces_per_subgraph: &HashMap<String, i64>,
    address: String,
    nonce: i64,
) -> HashMap<std::string::String, i64> {
    let mut updated_nonces = HashMap::new();
    updated_nonces.clone_from(nonces_per_subgraph);
    updated_nonces.insert(address, nonce);
    updated_nonces
}

#[derive(Clone, Debug)]
pub struct MessageWithCtx {
    pub message: GraphcastMessage,
    pub sender: String,
    pub sender_stake: BigUint,
}

impl MessageWithCtx {
    pub async fn new(message: GraphcastMessage) -> Result<Self, anyhow::Error> {
        let radio_payload =
            RadioPayloadMessage::new(message.identifier.clone(), message.content.clone());
        let address = format!(
            "{:#x}",
            Signature::from_str(&message.signature)?.recover(radio_payload.encode_eip712()?)?
        );
        let sender =
            query_registry_indexer(REGISTRY_SUBGRAPH.to_string(), address.to_string()).await?;

        let stake = query_network_subgraph(NETWORK_SUBGRAPH.to_string(), address.clone())
            .await?
            .indexer_stake();

        Ok(Self {
            message,
            sender,
            sender_stake: stake,
        })
    }

    /// Check message from valid sender: resolve indexer address and self stake
    pub async fn valid_sender(&self) -> Result<&Self, anyhow::Error> {
        if query_network_subgraph(NETWORK_SUBGRAPH.to_string(), self.sender.clone())
            .await?
            .stake_satisfy_requirement()
        {
            println!("Valid Indexer:  {}", self.sender);
            Ok(self)
        } else {
            Err(anyhow!(
                "Sender stake is less than the minimum requirement, drop message"
            ))
        }
    }

    /// Check timestamp: prevent past message replay
    pub fn valid_time(&self) -> Result<&Self, anyhow::Error> {
        //Can store for measuring overall gossip message latency
        let message_age = Utc::now().timestamp() - self.message.nonce;
        // 0 allow instant atomic messaging, use 1 to exclude them
        if (0..MSG_REPLAY_LIMIT).contains(&message_age) {
            Ok(self)
        } else {
            Err(anyhow!(
                "Message timestamp {} outside acceptable range {}, drop message",
                message_age,
                MSG_REPLAY_LIMIT
            ))
        }
    }

    /// Check timestamp: prevent messages with incorrect provider
    pub fn valid_hash(&self, block_hash: String) -> Result<&MessageWithCtx, anyhow::Error> {
        if self.message.block_hash == block_hash {
            Ok(self)
        } else {
            Err(anyhow!(
                "Message hash ({}) differ from trusted provider response ({}), drop message",
                self.message.block_hash,
                block_hash
            ))
        }
    }

    /// Check historic nonce: ensure message sequencing
    pub fn valid_nonce(&self, nonces: &Arc<Mutex<NoncesMap>>) -> Result<&Self, anyhow::Error> {
        let mut nonces = nonces.lock().unwrap();
        let nonces_per_subgraph = nonces.get(self.message.identifier.clone().as_str());

        match nonces_per_subgraph {
            Some(nonces_per_subgraph) => {
                let nonce = nonces_per_subgraph.get(&self.sender);
                match nonce {
                    Some(nonce) => {
                        println!(
                            "Latest saved nonce for subgraph {} and address {}: {}",
                            self.message.identifier, self.sender, nonce
                        );

                        if nonce > &self.message.nonce {
                            Err(anyhow!(
                                "Invalid nonce for subgraph {} and address {}! Received nonce - {} is smaller than currently saved one - {}, skipping message...",
                                self.message.identifier, self.sender, self.message.nonce, nonce
                            ))
                        } else {
                            let updated_nonces = prepare_nonces(
                                nonces_per_subgraph,
                                self.sender.clone(),
                                self.message.nonce,
                            );
                            nonces.insert(self.message.identifier.clone(), updated_nonces);
                            Ok(self)
                        }
                    }
                    None => {
                        let updated_nonces = prepare_nonces(
                            nonces_per_subgraph,
                            self.sender.clone(),
                            self.message.nonce,
                        );
                        nonces.insert(self.message.identifier.clone(), updated_nonces);
                        Err(anyhow!(
                                "No saved nonce for address {} on topic {}, saving this one and skipping message...",
                                self.sender, self.message.identifier
                            ))
                    }
                }
            }
            None => {
                let updated_nonces =
                    prepare_nonces(&HashMap::new(), self.sender.clone(), self.message.nonce);
                nonces.insert(self.message.identifier.clone(), updated_nonces);
                Err(anyhow!(
                        "First time receiving message for subgraph {}. Saving sender and nonce, skipping message...",
                        self.message.identifier
                    ))
            }
        }
    }
}

/// GraphcastMessage type casts over radio payload
#[derive(Eip712, EthAbiType, Clone, Message, Serialize, Deserialize)]
#[eip712(
    name = "GraphcastMessage",
    version = "1",
    chain_id = 1,
    verifying_contract = "0x0000000000000000000000000000000000000000"
)]
pub struct GraphcastMessage {
    /// Graph identifier for the entity the radio is communicating about
    #[prost(string, tag = "1")]
    pub identifier: String,
    /// content to share about the identified entity
    #[prost(string, tag = "2")]
    pub content: String,
    /// nonce cached to check against the next incoming message
    #[prost(int64, tag = "3")]
    pub nonce: i64,
    /// block relevant to the message
    #[prost(uint64, tag = "4")]
    pub block_number: u64,
    /// block hash generated from the block number
    #[prost(string, tag = "5")]
    pub block_hash: String,
    /// signature over radio payload
    #[prost(string, tag = "6")]
    pub signature: String,
}

impl GraphcastMessage {
    /// Create a graphcast message
    pub fn new(
        identifier: String,
        content: String,
        nonce: i64,
        block_number: i64,
        block_hash: String,
        signature: String,
    ) -> Self {
        GraphcastMessage {
            identifier,
            content,
            nonce,
            block_number: block_number.try_into().unwrap(),
            block_hash,
            signature,
        }
    }

    /// Signs the radio payload and construct graphcast message
    pub async fn build(
        wallet: &Wallet<SigningKey>,
        identifier: String,
        content: String,
        block_number: i64,
        block_hash: String,
    ) -> Result<Self, Box<dyn Error>> {
        println!("\n{}", "Constructing message".green());
        let sig = wallet
            .sign_typed_data(&RadioPayloadMessage::new(
                identifier.clone(),
                content.clone(),
            ))
            .await?;

        let message = GraphcastMessage::new(
            identifier.clone(),
            content,
            Utc::now().timestamp(),
            block_number,
            block_hash.to_string(),
            sig.to_string(),
        );

        println!("{}{:#?}", "Encode message: ".cyan(), message,);
        Ok(message)
    }

    /// Send Graphcast message to the Waku relay network
    pub fn send_to_waku(
        &self,
        node_handle: &WakuNodeHandle<Running>,
        pub_sub_topic: Option<WakuPubSubTopic>,
        content_topic: &WakuContentTopic,
    ) -> Result<String, Box<dyn Error>> {
        let mut buff = Vec::new();
        Message::encode(self, &mut buff).expect("Could not encode :(");

        let waku_message = WakuMessage::new(
            buff,
            content_topic.clone(),
            2,
            Utc::now().timestamp() as usize,
        );

        Ok(node_handle.relay_publish_message(&waku_message, pub_sub_topic, None)?)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dummy_message() {
        let hash: String = "Qmtest".to_string();
        let content: String = "0x0000".to_string();
        let nonce: i64 = 123321;
        let block_number: i64 = 0;
        let block_hash: String = "0xblahh".to_string();
        let sig: String = "signhere".to_string();
        let msg = GraphcastMessage::new(
            hash,
            content,
            nonce,
            block_number,
            block_hash.clone(),
            sig.clone(),
        );

        assert!(MessageWithCtx::new(msg).await.is_err());
    }

    #[tokio::test]
    async fn test_signed_message() {
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

        let msg_with_ctx = MessageWithCtx::new(msg).await.unwrap();
        let msg = &msg_with_ctx.valid_sender().await.unwrap().message;
        assert_eq!(msg.content, content);
    }
}
