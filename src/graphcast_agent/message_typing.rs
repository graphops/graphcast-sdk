use anyhow::anyhow;
use async_graphql::SimpleObject;
use chrono::Utc;
use ethers::signers::{Signer, Wallet};
use ethers_core::{
    k256::ecdsa::SigningKey,
    types::{transaction::eip712::Eip712Error, Signature},
};
use prost::Message;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt, str::FromStr, sync::Arc};
use tokio::sync::Mutex;
use tracing::{debug, error, trace};
use waku::{Running, WakuContentTopic, WakuMessage, WakuNodeHandle, WakuPubSubTopic};

use crate::{
    callbook::CallBook,
    graphql::{client_network::query_network_subgraph, QueryError},
    Account, NetworkBlockError, NoncesMap,
};

use super::{waku_handling::WakuHandlingError, MSG_REPLAY_LIMIT};

/// Prepare sender:nonce to update
fn prepare_nonces(
    nonces_per_subgraph: &HashMap<String, u64>,
    address: String,
    nonce: u64,
) -> HashMap<std::string::String, u64> {
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

//TODO: add required functions for RadioPayload, such as
// Build, new, validations, ...; may need to be async trait for valid checks
pub trait RadioPayload:
    Message
    + ethers::types::transaction::eip712::Eip712<Error = Eip712Error>
    + Default
    + Clone
    + 'static
    + Serialize
    + async_graphql::OutputType
{
    // type ExternalValidation;
    // async fn validity_check(&self, gc: GraphcastMessage<Self>, input: Self::ExternalValidation) -> Result<&Self, MessageError>;

    fn valid_outer(&self, outer: &GraphcastMessage<Self>) -> Result<&Self, MessageError>;
}

/// GraphcastMessage type casts over radio payload
#[derive(Clone, Message, Serialize, Deserialize, SimpleObject)]
pub struct GraphcastMessage<T: RadioPayload> {
    /// Graph identifier for the entity the radio is communicating about
    #[prost(string, tag = "1")]
    pub identifier: String,
    /// nonce cached to check against the next incoming message
    #[prost(uint64, tag = "3")]
    pub nonce: u64,
    /// Graph account sender
    #[prost(string, tag = "4")]
    pub graph_account: String,
    /// content to share about the identified entity
    #[prost(message, required, tag = "2")]
    pub payload: T,
    /// signature over radio payload
    #[prost(string, tag = "5")]
    pub signature: String,
}

impl<T: RadioPayload> GraphcastMessage<T> {
    /// Create a graphcast message
    pub fn new(
        identifier: String,
        nonce: u64,
        graph_account: String,
        payload: T,
        signature: String,
    ) -> Result<Self, MessageError> {
        Ok(GraphcastMessage {
            identifier,
            nonce,
            graph_account,
            payload,
            signature,
        })
    }

    /// Signs the radio payload and construct graphcast message
    pub async fn build(
        wallet: &Wallet<SigningKey>,
        identifier: String,
        graph_account: String,
        nonce: u64,
        payload: T,
    ) -> Result<Self, MessageError> {
        let sig = wallet
            .sign_typed_data(&payload)
            .await
            .map_err(|_| MessageError::Signing)?;

        GraphcastMessage::new(identifier, nonce, graph_account, payload, sig.to_string())
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

        node_handle
            .relay_publish_message(&waku_message, Some(pubsub_topic.clone()), None)
            .map_err(|e| {
                debug!(
                    error = tracing::field::debug(&e),
                    "Failed to relay publish the message"
                );
                WakuHandlingError::PublishMessage(e)
            })
    }

    /// Check message from valid sender: resolve indexer address and self stake
    pub async fn valid_sender(
        &self,
        registry_subgraph: &str,
        network_subgraph: &str,
        local_sender_id: String,
        id_validation: &IdentityValidation,
    ) -> Result<&Self, MessageError> {
        if id_validation == &IdentityValidation::NoCheck {
            return Ok(self);
        };
        trace!(id = tracing::field::debug(&id_validation), "Check account");

        let _ = self
            .remote_account(local_sender_id)?
            .verify(network_subgraph, registry_subgraph, id_validation)
            .await?;
        Ok(self)
    }

    /// Check timestamp: prevent past message replay
    pub fn valid_time(&self) -> Result<&Self, MessageError> {
        let current_time = Utc::now().timestamp();
        let current_time_u64 = current_time as u64;

        match current_time_u64.checked_sub(self.nonce) {
            Some(message_age) if (0..MSG_REPLAY_LIMIT).contains(&message_age) => Ok(self),
            Some(message_age) => Err(MessageError::InvalidFields(anyhow!(
                "Message timestamp {} outside acceptable range {}, drop message",
                message_age,
                MSG_REPLAY_LIMIT
            ))),
            None => Err(MessageError::InvalidFields(anyhow!(
                "Error calculating message age, possible underflow."
            ))),
        }
    }

    pub fn remote_account(&self, local_sender_id: String) -> Result<Account, MessageError> {
        let sender_address = self.recover_sender_address().and_then(|a| {
            trace!("recovered sender address: {:#?}\n", a,);
            if a != local_sender_id {
                Ok(a)
            } else {
                Err(MessageError::InvalidFields(anyhow!(
                    "Message is from self, drop message"
                )))
            }
        })?;
        Ok(Account::new(sender_address, self.graph_account.clone()))
    }

    /// Recover sender address from Graphcast message radio payload
    pub fn recover_sender_address(&self) -> Result<String, MessageError> {
        let signed_data = self
            .payload
            .encode_eip712()
            .expect("Could not encode payload using EIP712");
        match Signature::from_str(&self.signature).and_then(|sig| sig.recover(signed_data)) {
            Ok(addr) => Ok(format!("{addr:#x}")),
            Err(x) => Err(MessageError::InvalidFields(x.into())),
        }
    }

    /// Check historic nonce: ensure message sequencing
    pub async fn valid_nonce(&self, nonces: &Arc<Mutex<NoncesMap>>) -> Result<&Self, MessageError> {
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
                            Err(MessageError::InvalidFields(anyhow!(
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
                        Err(MessageError::InvalidFields(anyhow!(
                                    "No saved nonce for address {} on topic {}, saving this one and skipping message...",
                                    address, self.identifier
                                )))
                    }
                }
            }
            None => {
                let updated_nonces = prepare_nonces(&HashMap::new(), address, self.nonce);
                nonces.insert(self.identifier.clone(), updated_nonces);
                Err(MessageError::InvalidFields(anyhow!(
                            "First time receiving message for subgraph {}. Saving sender and nonce, skipping message...",
                            self.identifier
                        )))
            }
        }
    }

    pub fn decode(payload: &[u8]) -> Result<Self, WakuHandlingError> {
        <GraphcastMessage<T> as Message>::decode(payload).map_err(|e| {
            WakuHandlingError::InvalidMessage(format!(
                "Waku message not interpretated as a Graphcast message\nError occurred: {e:?}"
            ))
        })
    }
}

/// Check validity of the message:
/// Sender check verifies sender's on-chain identity with Graphcast registry
/// Time check verifies that message was from within the acceptable timestamp
/// Block hash check verifies sender's access to valid Ethereum node provider and blocks
/// Nonce check ensures the ordering of the messages and avoids past messages
pub async fn check_message_validity<T: RadioPayload>(
    graphcast_message: GraphcastMessage<T>,
    nonces: &Arc<Mutex<NoncesMap>>,
    callbook: CallBook,
    local_sender_id: String,
    id_validation: &IdentityValidation,
) -> Result<GraphcastMessage<T>, MessageError> {
    graphcast_message
        .valid_sender(
            callbook.graphcast_registry(),
            callbook.graph_network(),
            local_sender_id,
            id_validation,
        )
        .await?
        .valid_time()?
        .valid_nonce(nonces)
        .await?;

    trace!(
        message = tracing::field::debug(&graphcast_message),
        "Valid message!"
    );
    Ok(graphcast_message)
}

#[derive(Debug, thiserror::Error)]
pub enum MessageError {
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

impl MessageError {
    pub fn type_string(&self) -> &'static str {
        match self {
            MessageError::Payload => "Payload",
            MessageError::Signing => "Signing",
            MessageError::Encoding => "Encoding",
            MessageError::Decoding => "Decoding",
            MessageError::InvalidFields(_) => "InvalidFields",
            MessageError::Network(_) => "Network",
            MessageError::FieldDerivations(_) => "FieldDerivations",
            MessageError::TypeCast(_) => "TypeCast",
        }
    }
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
    // valid Graph indexer, Graphcast Registered Indexer, or Message identifier owner / subgraph owner
    // Does not include Curator or Delegator
    SubgraphStaker,
}

impl fmt::Display for IdentityValidation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdentityValidation::NoCheck => write!(f, "no-check"),
            IdentityValidation::ValidAddress => write!(f, "valid-address"),
            IdentityValidation::GraphcastRegistered => write!(f, "graphcast-registered"),
            IdentityValidation::GraphNetworkAccount => write!(f, "graph-network-account"),
            IdentityValidation::RegisteredIndexer => write!(f, "registered-indexer"),
            IdentityValidation::Indexer => write!(f, "indexer"),
            IdentityValidation::SubgraphStaker => write!(f, "subgraph-staker"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::wallet_address;

    use super::*;
    use ethers_contract::EthAbiType;
    use ethers_core::rand::thread_rng;
    use ethers_core::types::transaction::eip712::Eip712;
    use ethers_derive_eip712::*;
    use serde::{Deserialize, Serialize};

    /// Make a test radio type
    #[derive(Eip712, EthAbiType, Clone, Message, Serialize, Deserialize, SimpleObject)]
    #[eip712(
        name = "Graphcast Ping-Pong Radio",
        version = "0",
        chain_id = 1,
        verifying_contract = "0xc944e90c64b2c07662a292be6244bdf05cda44a7"
    )]
    pub struct SimpleMessage {
        #[prost(string, tag = "1")]
        pub identifier: String,
        #[prost(string, tag = "2")]
        pub content: String,
    }

    impl RadioPayload for SimpleMessage {
        //TODO: Add various requirements to RadioPayload
        // type ExternalValidation = Option<String>;
        // fn validity_check(&self, _gc: GraphcastMessage<Self>, _val: Option<String>) -> Result<&Self, MessageError> {
        //     Ok(self)
        // }
        fn valid_outer(&self, outer: &GraphcastMessage<Self>) -> Result<&Self, MessageError> {
            if self.identifier == outer.identifier {
                Ok(self)
            } else {
                Err(MessageError::InvalidFields(anyhow::anyhow!(
                    "Radio message wrapped by inconsistent GraphcastMessage: {:#?} <- {:#?}",
                    &self,
                    &outer,
                )))
            }
        }
    }

    impl SimpleMessage {
        pub fn new(identifier: String, content: String) -> Self {
            SimpleMessage {
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

    // Signature generated from goerli main indexer account
    fn graph_account_message() -> GraphcastMessage<SimpleMessage> {
        GraphcastMessage {
            identifier: String::from("ping-pong-content-topic"),
            nonce: 1688744240,
            graph_account: String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f"),
            payload:
                SimpleMessage {
                    identifier: String::from("table"),
                    content: String::from("Ping") },
            signature: String::from("a68733f919065a7eab215add3b0dc9cfb2d63b00fcd310803e8ee2dc9cf034af03f6fa4ba431e3d6167156d604e1dea2136bb3fea6d290ac6db980b30f790acb1c")
        }
    }

    // Signature generated from goerli secondary indexer account
    fn indexer_message() -> GraphcastMessage<SimpleMessage> {
        GraphcastMessage {
            identifier: String::from("ping-pong-content-topic"), 
            nonce: 1688743340,
            graph_account: String::from("0x6121d1036d7016b125f019268b0406a4c15bb99d"), 
            payload:
             SimpleMessage { identifier: String::from("table"), content: String::from("Ping") },
            signature: String::from("de8b176cb78aa2ec0bc9e163374423309cba10947fed04b5544bd9db81f54ded66328486e959771372ea5e8c093fe80dea64b7d3004bc59cd14712721208fab01b"),
        }
    }

    // Signature generated from goerli third graph account
    fn graphcast_id_message() -> GraphcastMessage<SimpleMessage> {
        GraphcastMessage {
            identifier: String::from("ping-pong-content-topic"),
            nonce: 1688742308,
            graph_account: String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f"),
            payload:
                SimpleMessage {
                    identifier: String::from("table"),
                    content: String::from("Ping") },
            signature: String::from("60a4b735acaf0c2490a51e34e0b799080c5c144ee2fe5dc9499465c490a4c5e946609c7d27d3b39cf4110d4f9402bac7f89cf2bd3850ae816506e638cde1a3c11c")
        }
    }

    #[tokio::test]
    async fn test_signature() {
        let wallet = dummy_wallet();
        let msg = GraphcastMessage::build(
            &wallet,
            String::from("ping-pong-content-topic"),
            String::from("0xe9a1cabd57700b17945fd81feefba82340d9568f"),
            1688742308,
            SimpleMessage {
                identifier: String::from("table"),
                content: String::from("Ping"),
            },
        )
        .await
        .unwrap();

        assert!(wallet_address(&wallet) == msg.recover_sender_address().unwrap());
    }

    #[tokio::test]
    async fn test_standard_message() {
        let registry_subgraph =
            "https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test";
        let network_subgraph =
            "https://api.thegraph.com/subgraphs/name/graphprotocol/graph-network-goerli";

        let hash: String = "table".to_string();
        let content: String = "Ping".to_string();
        let payload: SimpleMessage = SimpleMessage::new(hash.clone(), content.clone());
        let nonce = Utc::now().timestamp() as u64;

        let wallet = dummy_wallet();
        let msg = GraphcastMessage::build(&wallet, hash, wallet_address(&wallet), nonce, payload)
            .await
            .expect("Could not build message");

        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "x".to_string(),
                &IdentityValidation::ValidAddress
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "x".to_string(),
                &IdentityValidation::RegisteredIndexer
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
                &IdentityValidation::NoCheck
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::ValidAddress
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::GraphNetworkAccount
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::Indexer
            )
            .await
            .is_ok());

        // Message should fail to validate if registry is required
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::GraphcastRegistered
            )
            .await
            .is_err());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::RegisteredIndexer
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
                &IdentityValidation::NoCheck
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::ValidAddress
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::GraphNetworkAccount
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::Indexer
            )
            .await
            .is_ok());

        // Message should fail to validate if registry is required
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::GraphcastRegistered
            )
            .await
            .is_err());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::RegisteredIndexer
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
                &IdentityValidation::NoCheck
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::ValidAddress
            )
            .await
            .is_ok());

        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::Indexer
            )
            .await
            .is_ok());

        // Message should fail to validate if only Graph network account is checked
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::GraphNetworkAccount
            )
            .await
            .is_err());

        // Should success for checks at Graphcast registry
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::GraphcastRegistered
            )
            .await
            .is_ok());
        assert!(msg
            .valid_sender(
                registry_subgraph,
                network_subgraph,
                "".to_string(),
                &IdentityValidation::RegisteredIndexer
            )
            .await
            .is_ok());
    }
}
