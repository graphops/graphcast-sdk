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

use once_cell::sync::OnceCell;
use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub mod gossip_agent;
pub mod graphql;

type NoncesMap = HashMap<String, HashMap<String, i64>>;

/// Each radio persist Nonces from sub-topic messages differentiating sender
pub static NONCES: OnceCell<Arc<Mutex<NoncesMap>>> = OnceCell::new();

/// Returns Graphcast application domain name
///
/// Example
///
/// ```
/// let mut f = graphcast_sdk::app_name();
/// assert_eq!(f, "graphcast".to_string());
/// ```
pub fn app_name() -> Cow<'static, str> {
    Cow::from("graphcast")
}

#[cfg(test)]
mod tests {
    use crate::gossip_agent::waku_handling::build_content_topics;

    #[test]
    fn test_build_content_topics() {
        let basics = ["Qmyumyum", "Ymqumqum"].to_vec();
        let res = build_content_topics("some-radio", 0, &basics);
        for i in 0..res.len() {
            assert_eq!(res[i].content_topic_name, basics[i]);
            assert_eq!(res[i].application_name, "some-radio");
        }
    }
}
