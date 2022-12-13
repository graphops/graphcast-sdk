use lazy_static::lazy_static;
use std::{collections::HashMap, sync::Mutex};

pub mod gossip_agent;
pub mod graphql;

type NoncesMap = HashMap<String, HashMap<String, i64>>;

lazy_static! {
    pub static ref NONCES: Mutex<NoncesMap> = {
        let m = HashMap::new();
        Mutex::new(m)
    };
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
