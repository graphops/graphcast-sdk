use std::{collections::HashMap, error::Error, str::FromStr};

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
use prost::Message;
use serde::{Deserialize, Serialize};
use waku::{Running, WakuContentTopic, WakuMessage, WakuNodeHandle, WakuPubSubTopic};

use crate::{
    constants::{self, NETWORK_SUBGRAPH},
    graphql::client_network::query_network_subgraph,
    graphql::client_registry::query_registry_indexer,
    message_typing, NONCES,
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

    pub async fn build(
        wallet: &Wallet<SigningKey>,
        subgraph_hash: String,
        npoi: String,
        block_number: i64,
        block_hash: String,
    ) -> Result<Self, Box<dyn Error>> {
        println!("\n{}", "Constructing POI message".green());
        let sig = wallet
            .sign_typed_data(&RadioPayloadMessage::new(
                subgraph_hash.clone(),
                npoi.clone(),
            ))
            .await?;

        let message = GraphcastMessage::new(
            subgraph_hash.clone(),
            npoi,
            Utc::now().timestamp(),
            block_number,
            block_hash.to_string(),
            sig.to_string(),
        );

        println!("{}{:#?}", "Encode message: ".cyan(), message,);
        Ok(message)
    }

    pub fn send_to_waku(
        &self,
        node_handle: &WakuNodeHandle<Running>,
        pub_sub_topic: Option<WakuPubSubTopic>,
        poi_content_topic: WakuContentTopic,
    ) -> Result<String, Box<dyn Error>> {
        let mut buff = Vec::new();
        Message::encode(self, &mut buff).expect("Could not encode :(");

        let waku_message =
            WakuMessage::new(buff, poi_content_topic, 2, Utc::now().timestamp() as usize);

        Ok(node_handle.relay_publish_message(&waku_message, pub_sub_topic, None)?)
    }

    // Check message from valid sender: resolve indexer address and self stake
    pub async fn valid_sender(&self) -> Result<&GraphcastMessage, anyhow::Error> {
        let radio_payload =
            message_typing::RadioPayloadMessage::new(self.subgraph_hash.clone(), self.npoi.clone());
        let address = format!(
            "{:#x}",
            Signature::from_str(&self.signature)?.recover(radio_payload.encode_eip712()?)?
        );
        let indexer_address = query_registry_indexer(
            constants::REGISTRY_SUBGRAPH.to_string(),
            address.to_string(),
        )
        .await?;
        if query_network_subgraph(NETWORK_SUBGRAPH.to_string(), indexer_address.clone())
            .await?
            .stake_satisfy_requirement()
        {
            println!("Valid Indexer:  {}", indexer_address);
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

    pub fn valid_nonce(&self, nonces: &NONCES) -> Result<&GraphcastMessage, anyhow::Error> {
        let radio_payload =
            message_typing::RadioPayloadMessage::new(self.subgraph_hash.clone(), self.npoi.clone());
        let address = format!(
            "{:#x}",
            Signature::from_str(&self.signature)
                .unwrap()
                .recover(radio_payload.encode_eip712().unwrap())
                .unwrap()
        );

        let mut nonces = nonces.lock().unwrap();
        let nonces_per_subgraph = nonces.get(self.subgraph_hash.clone().as_str());

        match nonces_per_subgraph {
            Some(nonces_per_subgraph) => {
                let nonce = nonces_per_subgraph.get(&address);
                match nonce {
                    Some(nonce) => {
                        println!(
                            "Latest saved nonce for subgraph {} and address {}: {}",
                            self.subgraph_hash, address, nonce
                        );

                        if nonce > &self.nonce {
                            Err(anyhow!(
                            "Invalid nonce for subgraph {} and address {}! Received nonce - {} is smaller than currently saved one - {}, skipping message...",
                            self.subgraph_hash, address, self.nonce, nonce
                        ))
                        } else {
                            let updated_nonces = Self::prepare_nonces(
                                nonces_per_subgraph,
                                address.clone(),
                                self.nonce,
                            );
                            nonces.insert(self.subgraph_hash.clone(), updated_nonces);
                            Ok(self)
                        }
                    }
                    None => {
                        let updated_nonces =
                            Self::prepare_nonces(nonces_per_subgraph, address.clone(), self.nonce);
                        nonces.insert(self.subgraph_hash.clone(), updated_nonces);
                        Err(anyhow!(
                            "No saved nonce for address {} on topic {}, saving this one and skipping message...",
                            address, self.subgraph_hash
                        ))
                    }
                }
            }
            None => {
                let updated_nonces = Self::prepare_nonces(&HashMap::new(), address, self.nonce);
                nonces.insert(self.subgraph_hash.clone(), updated_nonces);
                Err(anyhow!(
                    "First time receiving message for subgraph {}. Saving sender and nonce, skipping message...",
                    self.subgraph_hash
                ))
            }
        }
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
        let npoi: String = "0x0000".to_string();
        let nonce: i64 = 123321;
        let block_number: i64 = 0;
        let block_hash: String = "0xblahh".to_string();
        let sig: String = "signhere".to_string();
        let msg = GraphcastMessage::new(
            hash,
            npoi,
            nonce,
            block_number,
            block_hash.clone(),
            sig.clone(),
        );

        assert_eq!(msg.block_number, 0);
        assert!(msg.valid_sender().await.is_err());
        assert!(msg.valid_time().is_err());
        assert!(msg.valid_hash("weeelp".to_string()).is_err());
        assert_eq!(
            msg.valid_hash("0xblahh".to_string()).unwrap().signature,
            sig
        );
    }

    #[tokio::test]
    async fn test_signed_message() {
        let hash: String = "QmWECgZdP2YMcV9RtKU41GxcdW8EGYqMNoG98ubu5RGN6U".to_string();
        let npoi: String =
            "0xa6008cea5905b8b7811a68132feea7959b623188e2d6ee3c87ead7ae56dd0eae".to_string();
        let nonce: i64 = 123321;
        let block_number: i64 = 0;
        let block_hash: String = "0xblahh".to_string();
        let sig: String = "4be6a6b7f27c4086f22e8be364cbdaeddc19c1992a42b08cbe506196b0aafb0a68c8c48a730b0e3155f4388d7cc84a24b193d091c4a6a4e8cd6f1b305870fae61b".to_string();
        let msg = GraphcastMessage::new(
            hash,
            npoi.clone(),
            nonce,
            block_number,
            block_hash.clone(),
            sig,
        );

        assert_eq!(msg.valid_sender().await.unwrap().npoi, npoi);
    }
}
