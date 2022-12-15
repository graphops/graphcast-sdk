//! # Graphcast
//!
//! `graphcast` is a development kit to build The Graph network
//! gossip p2p messaging apps on top of the Waku Relay network.
//!
//! This library is a work in progress, in particular
//! operating on Goerli testnet and Waku Rust Bindings.
//!
//! ## Getting Started
//!
//! Add this crate to your project's `Cargo.toml`.
//!
//! ## Examples and Usage
//!
//! Check out the examples folder for helpful snippets of code, as well as minimal configurations.
//! For more explanation, see the crate documentation.
//!

use num_bigint::BigUint;
use once_cell::sync::OnceCell;
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub mod gossip_agent;
pub mod graphql;
pub enum Sender {
    Indexer { address: String, stake: BigUint },
}

type NoncesMap = HashMap<String, HashMap<String, i64>>;

/// Each radio persist Nonces from sub-topic messages differentiating sender
pub static NONCES: OnceCell<Arc<Mutex<NoncesMap>>> = OnceCell::new();

/// Returns Graphcast application domain name
///
/// Example
///
/// ```
/// let mut f = graphcast::app_name();
/// assert_eq!(f, "graphcast".to_string());
/// ```
pub fn app_name() -> Cow<'static, str> {
    Cow::from("graphcast")
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use crate::gossip_agent::waku_handling::generate_pubsub_topics;

    #[test]
    fn test_generate_pubsub_topics() {
        let basics = ["Qmyumyum".to_string(), "Ymqumqum".to_string()].to_vec();
        let basics_generated: Vec<Cow<'static, str>> = [
            Cow::from("graphcast-some-radio-Qmyumyum"),
            Cow::from("graphcast-some-radio-Ymqumqum"),
        ]
        .to_vec();
        let res = generate_pubsub_topics("some-radio", &basics);
        for i in 0..res.len() {
            assert_eq!(res[i].as_ref().unwrap().topic_name, basics_generated[i]);
        }
    }
}
