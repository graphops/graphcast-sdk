use std::str::FromStr;

use crate::{message_typing::{GraphcastMessage, self}, constants, graphql::client_registry::query_registry_indexer};
use ethers::types::transaction::eip712::Eip712;
use ethers_core::types::Signature;

pub async fn resolve_indexer_address(msg: &GraphcastMessage) -> Result<String, anyhow::Error> {
    let radio_payload =
        message_typing::RadioPayloadMessage::new(msg.subgraph_hash.clone(), msg.npoi.clone());
    let address = format!(
        "{:#x}",
        Signature::from_str(&msg.signature)?.recover(radio_payload.encode_eip712()?)?
    );
    let indexer_address = query_registry_indexer(
        constants::REGISTRY_SUBGRAPH.to_string(),
        address.to_string(),
    )
    .await;

    indexer_address
}
