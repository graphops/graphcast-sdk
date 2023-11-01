use async_graphql::SimpleObject;
use ethers_contract::EthAbiType;
use ethers_core::types::transaction::eip712::Eip712;
use ethers_derive_eip712::*;

use graphcast_sdk::graphcast_agent::message_typing::{
    GraphcastMessage, MessageError, RadioPayload,
};
use prost::Message;
use serde::{Deserialize, Serialize};

// Import the OnceCell container for lazy initialization of global/static data
use once_cell::sync::OnceCell;
use std::sync::{Arc, Mutex};

/// A global static (singleton) instance of A GraphcastMessage vector.
/// It is used to save incoming messages after they've been validated, in order
/// defer their processing for later, because async code is required for the processing but
/// it is not allowed in the handler itself.
pub static MESSAGES: OnceCell<Arc<Mutex<Vec<SimpleMessage>>>> = OnceCell::new();

/// Make a test radio type
#[derive(Eip712, EthAbiType, Clone, Message, Serialize, Deserialize, SimpleObject)]
#[eip712(
    name = "SimpleMessage",
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

    pub fn radio_handler(&self) {
        MESSAGES
            .get()
            .expect("Could not retrieve messages")
            .lock()
            .expect("Could not get lock on messages")
            .push(self.clone());
    }
}
