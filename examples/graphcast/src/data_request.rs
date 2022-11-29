use waku::{Encoding, WakuPubSubTopic};

//TODO: refactor topic generation
pub async fn generate_pubsub_topics(
    app_name: String,
    items: Vec<String>,
) -> Vec<Option<WakuPubSubTopic>> {
    items
        .into_iter()
        .map(|hash| {
            let borrowed_hash: &str = &hash;
            let topic = app_name.clone() + "-poi-crosschecker-" + borrowed_hash;

            Some(WakuPubSubTopic {
                topic_name: topic,
                encoding: Encoding::Proto,
            })
        })
        .collect::<Vec<Option<WakuPubSubTopic>>>()
}
