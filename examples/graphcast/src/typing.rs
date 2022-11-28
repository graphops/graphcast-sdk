use ethers::types::RecoveryMessage;
use ethers_contract::EthAbiType;
use ethers_core::types::transaction::eip712::Eip712;
use ethers_derive_eip712::*;
use prost::Message;
use serde::{Deserialize, Serialize};

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
    pub subgraph_hash: String,
    #[prost(string, tag = "2")]
    pub npoi: String,
}

impl RadioPayloadMessage {
    pub fn new(subgraph_hash: String, npoi: String) -> Self {
        RadioPayloadMessage {
            subgraph_hash,
            npoi,
        }
    }
}
