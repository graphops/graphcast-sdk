use anyhow::anyhow;
use async_graphql::SimpleObject;
use chrono::Utc;
use ethers::signers::{Signer, Wallet};
use ethers_core::{k256::ecdsa::SigningKey, types::Signature};
use num_traits::ToPrimitive;
use prost::Message;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, str::FromStr, sync::Arc};
use tokio::sync::Mutex;
use tracing::{debug, error, trace};
use waku::{Running, WakuContentTopic, WakuMessage, WakuNodeHandle, WakuPeerData, WakuPubSubTopic};

use crate::{
    callbook::CallBook,
    graphql::{
        client_graph_node::query_graph_node_network_block_hash,
        client_network::query_network_subgraph, QueryError,
    },
    networks::NetworkName,
    Account, NetworkBlockError, NoncesMap,
};

use super::{waku_handling::WakuHandlingError, MSG_REPLAY_LIMIT};

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

pub async fn get_indexer_stake(
    indexer_address: &str,
    network_subgraph: &str,
) -> Result<f32, QueryError> {
    Ok(query_network_subgraph(network_subgraph, indexer_address)
        .await?
        .indexer_stake())
}

/// GraphcastMessage type casts over radio payload
#[derive(Clone, Message, Serialize, Deserialize, SimpleObject)]
pub struct GraphcastMessage<T>
where
    T: Message
        + ethers::types::transaction::eip712::Eip712
        + Default
        + Clone
        + 'static
        + async_graphql::OutputType,
{
    /// Graph identifier for the entity the radio is communicating about
    #[prost(string, tag = "1")]
    pub identifier: String,
    /// content to share about the identified entity
    #[prost(message, required, tag = "2")]
    pub payload: T,
    /// nonce cached to check against the next incoming message
    #[prost(int64, tag = "3")]
    pub nonce: i64,
    /// blockchain relevant to the message
    #[prost(string, tag = "4")]
    pub network: String,
    /// block relevant to the message
    #[prost(uint64, tag = "5")]
    pub block_number: u64,
    /// block hash generated from the block number
    #[prost(string, tag = "6")]
    pub block_hash: String,
    /// Graph account sender
    #[prost(string, tag = "7")]
    pub graph_account: String,
    /// signature over radio payload
    #[prost(string, tag = "8")]
    pub signature: String,
}

impl<
        T: Message
            + ethers::types::transaction::eip712::Eip712
            + Default
            + Clone
            + 'static
            + async_graphql::OutputType,
    > GraphcastMessage<T>
{
    /// Create a graphcast message
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        identifier: String,
        payload: T,
        nonce: i64,
        network: NetworkName,
        block_number: u64,
        block_hash: String,
        graph_account: String,
        signature: String,
    ) -> Result<Self, BuildMessageError> {
        if let Some(block_number) = block_number.to_u64() {
            Ok(GraphcastMessage {
                identifier,
                payload,
                nonce,
                network: network.to_string(),
                block_number,
                block_hash,
                graph_account,
                signature,
            })
        } else {
            Err(BuildMessageError::TypeCast(format!(
                "Error: Invalid block number {block_number}, conversion to u64 failed."
            )))
        }
    }

    /// Signs the radio payload and construct graphcast message
    pub async fn build(
        wallet: &Wallet<SigningKey>,
        identifier: String,
        payload: T,
        network: NetworkName,
        block_number: u64,
        block_hash: String,
        graph_account: String,
    ) -> Result<Self, BuildMessageError> {
        let sig = wallet
            .sign_typed_data(&payload)
            .await
            .map_err(|_| BuildMessageError::Signing)?;

        GraphcastMessage::new(
            identifier,
            payload,
            Utc::now().timestamp(),
            network,
            block_number,
            block_hash.to_string(),
            graph_account,
            sig.to_string(),
        )
    }

    /// Send Graphcast message to the Waku relay network
    pub fn send_to_waku(
        &self,
        node_handle: &WakuNodeHandle<Running>,
        pubsub_topic: WakuPubSubTopic,
        content_topic: WakuContentTopic,
    ) -> Result<String, WakuHandlingError> {
        let mut buff = Vec::new();
        Message::encode(self, &mut buff).expect("Could not encode :(");

        let waku_message = WakuMessage::new(
            buff,
            content_topic,
            2,
            Utc::now().timestamp() as usize,
            vec![],
            true,
        );
        trace!(message = tracing::field::debug(&self), "Sending message");

        let sent_result: Vec<Result<String, WakuHandlingError>> = node_handle
            .peers()
            .map_err(WakuHandlingError::RetrievePeersError)
            .unwrap_or_default()
            .iter()
            .filter(|&peer| {
                // Filter out local peer_id to prevent self dial
                peer.peer_id().as_str()
                    != node_handle
                        .peer_id()
                        .expect("Failed to find local node's peer id")
                        .as_str()
            })
            .map(|peer: &WakuPeerData| {
                // Filter subscribe to all other peers
                node_handle
                    .lightpush_publish(
                        &waku_message,
                        Some(pubsub_topic.clone()),
                        peer.peer_id().to_string(),
                        None,
                    )
                    .map_err(|e| {
                        debug!(
                            error = tracing::field::debug(&e),
                            "Failed to send message to Waku peer"
                        );
                        WakuHandlingError::PublishMessage(e)
                    })
            })
            .collect();
        // The message id is the same for all successful publish
        sent_result
            .into_iter()
            .find_map(|res| res.ok())
            .ok_or(WakuHandlingError::PublishMessage(
                "Message could not be sent to any peers".to_string(),
            ))
    }

    /// Check message from valid sender: resolve indexer address and self stake
    pub async fn valid_sender(
        &self,
        registry_subgraph: &str,
        network_subgraph: &str,
        local_sender_id: String,
        id_validation: IdentityValidation,
    ) -> Result<&Self, BuildMessageError> {
        match id_validation {
            IdentityValidation::NoCheck => (),
            IdentityValidation::ValidAddress => {
                let _ = self.remote_account(local_sender_id)?;
            }
            IdentityValidation::GraphcastRegistered => {
                let claimed_account = self.remote_account(local_sender_id)?;
                // Simply check if the message signer is registered at Graphcast Registry, make no validation on Graph Account field
                let _ = claimed_account
                    .account_from_registry(registry_subgraph)
                    .await
                    .map(|verified_account| {
                        if verified_account.account == claimed_account.account {Ok(verified_account)} else {
                            Err(BuildMessageError::InvalidFields(anyhow!("Failed to match signature with a Graph account by `GraphcastRegistered` validation mechanism, drop message")))
                        }
                    })
                    .map_err(BuildMessageError::FieldDerivations)?;
            }
            IdentityValidation::GraphNetworkAccount => {
                let claimed_account = self.remote_account(local_sender_id)?;
                // allow any Graph account matched with message signer and the self-claimed graph account
                let _ = claimed_account
                    .account_from_network(network_subgraph)
                    .await
                    .map(|verified_account| {
                        if verified_account.account == claimed_account.account {Ok(verified_account)} else {
                            Err(BuildMessageError::InvalidFields(anyhow!("Failed to match signature with a Graph account by `GraphcastRegistered` validation mechanism, drop message")))
                        }
                    })
                    .map_err(BuildMessageError::FieldDerivations)?;
            }
            IdentityValidation::RegisteredIndexer => {
                let claimed_account = self.remote_account(local_sender_id)?;
                let verified_account = claimed_account
                    .account_from_registry(registry_subgraph)
                    .await
                    .map_err(BuildMessageError::FieldDerivations)?;
                if verified_account.account() != claimed_account.account() {
                    return Err(BuildMessageError::InvalidFields(anyhow!("Failed to match signature with a Graph account by `RegisteredIndexer` validation mechanism, drop message")));
                };
                verified_account.valid_indexer(network_subgraph).await?;
            }
            IdentityValidation::Indexer => {
                let claimed_account = self.remote_account(local_sender_id)?;
                let verified_account = match claimed_account
                    .account_from_registry(registry_subgraph)
                    .await
                {
                    Ok(a) => a,
                    Err(e) => {
                        debug!(
                            e = tracing::field::debug(&e),
                            account = tracing::field::debug(&claimed_account),
                            "Signer is not registered at Graphcast Registry. Check Graph Network"
                        );
                        claimed_account
                            .account_from_network(network_subgraph)
                            .await
                            .map_err(BuildMessageError::FieldDerivations)?
                    }
                };
                if verified_account.account() != claimed_account.account() {
                    return Err(BuildMessageError::InvalidFields(anyhow!(format!("Failed to match signature with a Graph account by `Indexer` validation mechanism, drop message. Verified account: {:#?}\n account claimed by message: {:#?}", verified_account, claimed_account))));
                };
                let _ = verified_account.valid_indexer(network_subgraph).await;
            }
        };
        Ok(self)
    }

    /// Check timestamp: prevent past message replay
    pub fn valid_time(&self) -> Result<&Self, BuildMessageError> {
        //Can store for measuring overall Graphcast message latency
        let message_age = Utc::now().timestamp() - self.nonce;
        // 0 allow instant atomic messaging, use 1 to exclude them
        if (0..MSG_REPLAY_LIMIT).contains(&message_age) {
            Ok(self)
        } else {
            Err(BuildMessageError::InvalidFields(anyhow!(
                "Message timestamp {} outside acceptable range {}, drop message",
                message_age,
                MSG_REPLAY_LIMIT
            )))
        }
    }

    pub fn remote_account(&self, local_sender_id: String) -> Result<Account, BuildMessageError> {
        debug!(
            "recovered sender address: {:#?}\nlocal sender id: {:#?}",
            self.recover_sender_address(),
            local_sender_id
        );
        let sender_address = self.recover_sender_address().and_then(|a| {
            debug!(
                "recovered sender address: {:#?}\nlocal sender id: {:#?}",
                a,
                local_sender_id.clone()
            );
            if a != local_sender_id {
                Ok(a)
            } else {
                Err(BuildMessageError::InvalidFields(anyhow!(
                    "Message is from self, drop message"
                )))
            }
        })?;
        Ok(Account::new(sender_address, self.graph_account.clone()))
    }

    /// Check timestamp: prevent messages with incorrect graph node's block provider
    pub async fn valid_hash(&self, graph_node_endpoint: &str) -> Result<&Self, BuildMessageError> {
        let block_hash: String = query_graph_node_network_block_hash(
            graph_node_endpoint,
            &self.network,
            self.block_number,
        )
        .await
        .map_err(BuildMessageError::FieldDerivations)?;

        trace!(
            network = tracing::field::debug(self.network.clone()),
            block_number = self.block_number,
            block_hash = block_hash,
            "Queried block hash from graph node",
        );

        if self.block_hash == block_hash {
            Ok(self)
        } else {
            Err(BuildMessageError::InvalidFields(anyhow!(
                "Message hash ({}) differ from trusted provider response ({}), drop message",
                self.block_hash,
                block_hash
            )))
        }
    }

    /// Recover sender address from Graphcast message radio payload
    pub fn recover_sender_address(&self) -> Result<String, BuildMessageError> {
        let signed_data = self
            .payload
            .encode_eip712()
            .expect("Could not encode payload using EIP712");
        match Signature::from_str(&self.signature).and_then(|sig| sig.recover(signed_data)) {
            Ok(addr) => {
                debug!("{}", format!("{addr:#x}"));
                Ok(format!("{addr:#x}"))
            }
            Err(x) => Err(BuildMessageError::InvalidFields(x.into())),
        }
    }

    /// Check historic nonce: ensure message sequencing
    pub async fn valid_nonce(
        &self,
        nonces: &Arc<Mutex<NoncesMap>>,
    ) -> Result<&Self, BuildMessageError> {
        let address = self.recover_sender_address()?;

        let mut nonces = nonces.lock().await;
        let nonces_per_subgraph = nonces.get(&self.identifier);

        match nonces_per_subgraph {
            Some(nonces_per_subgraph) => {
                let nonce = nonces_per_subgraph.get(&address);
                match nonce {
                    Some(nonce) => {
                        trace!(
                            subgraph = self.identifier,
                            sender = address,
                            saved_nonce = nonce,
                            "Nonce check",
                        );

                        if nonce > &self.nonce {
                            Err(BuildMessageError::InvalidFields(anyhow!(
                                    "Invalid nonce for subgraph {} and address {}! Received nonce - {} is smaller than currently saved one - {}, skipping message...",
                                    self.identifier, address, self.nonce, nonce
                                )))
                        } else {
                            let updated_nonces =
                                prepare_nonces(nonces_per_subgraph, address, self.nonce);
                            nonces.insert(self.identifier.clone(), updated_nonces);
                            Ok(self)
                        }
                    }
                    None => {
                        let updated_nonces =
                            prepare_nonces(nonces_per_subgraph, address.clone(), self.nonce);
                        nonces.insert(self.identifier.clone(), updated_nonces);
                        Err(BuildMessageError::InvalidFields(anyhow!(
                                    "No saved nonce for address {} on topic {}, saving this one and skipping message...",
                                    address, self.identifier
                                )))
                    }
                }
            }
            None => {
                let updated_nonces = prepare_nonces(&HashMap::new(), address, self.nonce);
                nonces.insert(self.identifier.clone(), updated_nonces);
                Err(BuildMessageError::InvalidFields(anyhow!(
                            "First time receiving message for subgraph {}. Saving sender and nonce, skipping message...",
                            self.identifier
                        )))
            }
        }
    }
}

/// Check validity of the message:
/// Sender check verifies sender's on-chain identity with Graphcast registry
/// Time check verifies that message was from within the acceptable timestamp
/// Block hash check verifies sender's access to valid Ethereum node provider and blocks
/// Nonce check ensures the ordering of the messages and avoids past messages
pub async fn check_message_validity<
    T: Message
        + ethers::types::transaction::eip712::Eip712
        + Default
        + Clone
        + 'static
        + async_graphql::OutputType,
>(
    graphcast_message: GraphcastMessage<T>,
    nonces: &Arc<Mutex<NoncesMap>>,
    callbook: CallBook,
    local_sender_id: String,
    id_validation: IdentityValidation,
) -> Result<GraphcastMessage<T>, BuildMessageError> {
    graphcast_message
        .valid_sender(
            callbook.graphcast_registry(),
            callbook.graph_network(),
            local_sender_id,
            id_validation,
        )
        .await?
        .valid_time()?
        .valid_hash(callbook.graph_node_status())
        .await?
        .valid_nonce(nonces)
        .await?;

    trace!(
        message = tracing::field::debug(&graphcast_message),
        "Valid message!"
    );
    Ok(graphcast_message)
}

#[derive(Debug, thiserror::Error)]
pub enum BuildMessageError {
    #[error("Radio payload failed to satisfy the defined Eip712 typing")]
    Payload,
    #[error("Could not sign payload")]
    Signing,
    #[error("Could not encode message")]
    Encoding,
    #[error("Could not decode message")]
    Decoding,
    #[error("Could not pass message validity checks: {0}")]
    InvalidFields(anyhow::Error),
    #[error("Could not build message with Network and BlockPointer: {0}")]
    Network(NetworkBlockError),
    #[error("Could not derive fields from the existing message: {0}")]
    FieldDerivations(QueryError),
    #[error("{0}")]
    TypeCast(String),
}

/// Identity validation for a Graphcast Message
#[derive(Clone, Debug, Eq, PartialEq, Default, clap::ValueEnum, Serialize, Deserialize)]
pub enum IdentityValidation {
    // no checks
    NoCheck,
    // valid address
    ValidAddress,
    // valid Graphcast id
    GraphcastRegistered,
    // valid Graph Account
    GraphNetworkAccount,
    // valid Graphcast registered indexer
    #[default]
    RegisteredIndexer,
    // valid Graph indexer or Graphcast Registered Indexer
    Indexer,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers_contract::EthAbiType;
    use ethers_core::rand::thread_rng;
    use ethers_core::types::transaction::eip712::Eip712;
    use ethers_derive_eip712::*;
    use prost::Message;
    use serde::{Deserialize, Serialize};

    /// Make a test radio type
    #[derive(Eip712, EthAbiType, Clone, Message, Serialize, Deserialize, SimpleObject)]
    #[eip712(
        name = "Graphcast Ping-Pong Radio",
        version = "0",
        chain_id = 1,
        verifying_contract = "0xc944e90c64b2c07662a292be6244bdf05cda44a7"
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

        pub fn content_string(&self) -> &str {
            &self.content
        }
    }

    /// Create a random wallet
    fn dummy_wallet() -> Wallet<SigningKey> {
        Wallet::new(&mut thread_rng())
    }

    fn graph_account_message() -> GraphcastMessage<RadioPayloadMessage> {
        GraphcastMessage {
            identifier: String::from("ping-pong-content-topic"), 
            payload:
                RadioPayloadMessage {
                    identifier: String::from("table"), 
                    content: String::from("Ping") }, 
            nonce: 1687448729,
            network: String::from("goerli"),
            block_number: 9221945,
            block_hash: String::from("a8ad1057882ae2bce4e49f811e651ccacd317f3c11918d3724d7e7a551c5fc39"),
            graph_account: String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f"),
            signature: String::from("2cd3fa305efd9c362bc71adee6e5a85c357a951af84c80667b8ddae23ac81c3821dac7d9c167e2776a9a56d8726b472312f40d9cc7461d1a6950d00e52d6e8521b")
        }
    }

    fn indexer_message() -> GraphcastMessage<RadioPayloadMessage> {
        GraphcastMessage {
            identifier: String::from("ping-pong-content-topic"), 
            payload:
                RadioPayloadMessage {
                    identifier: String::from("table"), 
                    content: String::from("Ping") }, 
            nonce: 1687874581,
            network: String::from("goerli"),
            block_number: 9249797,
            block_hash: String::from("af04663a968f48a0bd554e5f4842b4f3546868f5d87221ae194e01d36f640cd0"),
            graph_account: String::from("0x6121d1036d7016b125f019268b0406a4c15bb99d"),
            signature: String::from("8006bd09f7ca6582ff1bbb9fd5bf657611625cd5a99f9d92088d9098c3391cd373454554bac8b76e13eb39b63be6d985761e76761c607bd2a87078259ab8928d1c")
        }
    }

    fn graphcast_id_message() -> GraphcastMessage<RadioPayloadMessage> {
        GraphcastMessage {
            identifier: String::from("ping-pong-content-topic"),
            payload:
                RadioPayloadMessage {
                    identifier: String::from("table"),
                    content: String::from("Ping") },
            nonce: 1687451299,
            network: String::from("goerli"),
            block_number: 9222109,
            block_hash: String::from("f1523bcac92c7e7d38142b089ec122d1607bc9a3b1b5d55df7cc11cbe10a3c48"),
            graph_account: String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f"),
            signature: String::from("52dcdd23418fa9c660be6c50f2c828c5b702ac46a452c21747260adc822a79663a3b7eddaa5139a0f5cd1206c8663faf272757d46f87bbb2bb6feedd1389601d1b")
        }
    }

    #[tokio::test]
    async fn test_standard_message() {
        let registry_subgraph =
            "https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test";
        let network_subgraph =
            "https://api.thegraph.com/subgraphs/name/graphprotocol/graph-network-goerli";
        let network = NetworkName::from_string("goerli");

        let hash: String = "Qmtest".to_string();
        let content: String = "0x0000".to_string();
        let payload: RadioPayloadMessage = RadioPayloadMessage::new(hash.clone(), content.clone());
        let block_number: u64 = 0;
        let block_hash: String = "0xblahh".to_string();

        let wallet = dummy_wallet();
        let msg = GraphcastMessage::build(
            &wallet,
            hash,
            payload,
            network,
            block_number,
            block_hash,
            String::from("0xE9a1CABd57700B17945Fd81feeFba82340D9568F"),
        )
        .await
        .expect("Could not build message");

        assert_eq!(msg.block_number, 0);
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::RegisteredIndexer
            )
            .await
            .is_err());
        assert!(msg.valid_time().is_ok());
        assert_eq!(msg.payload.content_string(), content);
        assert_eq!(
            msg.recover_sender_address()
                .expect("Could not recover sender address"),
            format!("{:#x}", wallet.address())
        );
    }

    #[tokio::test]
    async fn test_validate_graph_network() {
        let registry_subgraph =
            "https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test";
        let network_subgraph =
            "https://api.thegraph.com/subgraphs/name/graphprotocol/graph-network-goerli";
        // graph_account_message is by a valid eth address that is not registered as a graphcast_id but is a graph account and valid indexer
        let msg = graph_account_message();
        assert_eq!(
            msg.recover_sender_address().unwrap(),
            String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f")
        );
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::NoCheck
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::ValidAddress
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::GraphNetworkAccount
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::Indexer
            )
            .await
            .is_ok());

        // Message should fail to validate if registry is required
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::GraphcastRegistered
            )
            .await
            .is_err());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::RegisteredIndexer
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_validate_indexer() {
        let registry_subgraph =
            "https://thegraph.com/hosted-service/subgraph/hopeyen/graphcast-registry-goerli";
        let network_subgraph =
            "https://api.thegraph.com/subgraphs/name/graphprotocol/graph-network-goerli";
        // graph_account_message is by a valid eth address that is not registered as a graphcast_id but is a graph account and valid indexer
        let msg = indexer_message();
        assert_eq!(
            msg.recover_sender_address().unwrap(),
            String::from("0x6121d1036d7016b125f019268b0406a4c15bb99d")
        );
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::NoCheck
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::ValidAddress
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::GraphNetworkAccount
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::Indexer
            )
            .await
            .is_ok());

        // Message should fail to validate if registry is required
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::GraphcastRegistered
            )
            .await
            .is_err());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::RegisteredIndexer
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_validate_registry() {
        let registry_subgraph =
            "https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test";
        let network_subgraph =
            "https://api.thegraph.com/subgraphs/name/graphprotocol/graph-network-goerli";
        // graph_account_message is by a valid eth address that is not registered as a graphcast_id but is a graph account and valid indexer
        let msg = graphcast_id_message();
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::NoCheck
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::ValidAddress
            )
            .await
            .is_ok());

        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::Indexer
            )
            .await
            .is_ok());

        // Message should fail to validate if only Graph network account is checked
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::GraphNetworkAccount
            )
            .await
            .is_err());

        // Should success for checks at Graphcast registry
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::GraphcastRegistered
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                IdentityValidation::RegisteredIndexer
            )
            .await
            .is_ok());
    }
}
