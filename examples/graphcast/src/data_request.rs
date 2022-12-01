use waku::{Encoding, WakuPubSubTopic};

//TODO: refactor topic generation
pub fn generate_pubsub_topics(
    app_name: String,
    subtopic: &[String],
) -> Vec<Option<WakuPubSubTopic>> {
    (*subtopic
        .iter()
        .map(|hash| {
            let borrowed_hash: &str = hash;
            let topic = app_name.clone() + "-poi-crosschecker-" + borrowed_hash;

            Some(WakuPubSubTopic {
                topic_name: topic,
                encoding: Encoding::Proto,
            })
        })
        .collect::<Vec<Option<WakuPubSubTopic>>>())
    .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_topics() {
        let empty_vec = [].to_vec();
        let empty_topic_vec: Vec<Option<WakuPubSubTopic>> = [].to_vec();
        assert_eq!(
            generate_pubsub_topics(String::from("test"), &empty_vec).len(),
            empty_topic_vec.len()
        );
    }

    #[test]
    fn test_generate_pubsub_topics() {
        let basics = ["Qmyumyum".to_string(), "Ymqumqum".to_string()].to_vec();
        let basics_generated: Vec<String> = [
            "test-poi-crosschecker-Qmyumyum".to_string(),
            "test-poi-crosschecker-Ymqumqum".to_string(),
        ]
        .to_vec();
        let res = generate_pubsub_topics(String::from("test"), &basics);
        for i in 0..res.len() {
            assert_eq!(res[i].as_ref().unwrap().topic_name, basics_generated[i]);
        }
    }
}
