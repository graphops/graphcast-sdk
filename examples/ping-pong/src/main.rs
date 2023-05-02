use dotenv::dotenv;

use graphcast_sdk::{
    graphcast_agent::{
        message_typing::GraphcastMessage, waku_handling::WakuHandlingError, GraphcastAgent,
        GraphcastAgentConfig,
    },
    graphql::client_graph_node::{get_indexing_statuses, update_network_chainheads},
    networks::NetworkName,
    BlockPointer,
};
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::{thread::sleep, time::Duration};
use tokio::sync::Mutex as AsyncMutex;
use tracing::{debug, error, info};
use types::RadioPayloadMessage;

use crate::config::Config;

mod config;
mod types;

#[tokio::main]
async fn main() -> ! {
    // This can be any string
    let radio_name: &str = "ping-pong";
    // Loads the environment variables from .env
    dotenv().ok();

    /// A global static (singleton) instance of A GraphcastMessage vector.
    /// It is used to save incoming messages after they've been validated, in order
    /// defer their processing for later, because async code is required for the processing but
    /// it is not allowed in the handler itself.
    pub static MESSAGES: OnceCell<Arc<Mutex<Vec<GraphcastMessage<RadioPayloadMessage>>>>> =
        OnceCell::new();

    /// The Graphcast Agent instance must be a global static variable (for the time being).
    /// This is because the Radio handler requires a static immutable context and
    /// the handler itself is being passed into the Graphcast Agent, so it needs to be static as well.
    pub static GRAPHCAST_AGENT: OnceCell<GraphcastAgent> = OnceCell::new();

    let config = Config::args();

    // subtopics are optionally provided and used as the content topic identifier of the message subject,
    // if not provided then they are usually generated based on indexer allocations
    let subtopics = vec!["ping-pong-content-topic".to_string()];

    let graphcast_agent_config = GraphcastAgentConfig::new(
        config.private_key.expect("No private key provided"),
        radio_name,
        config.registry_subgraph,
        config.network_subgraph,
        config.graph_node_endpoint.clone(),
        None,
        Some("testnet".to_string()),
        Some(subtopics),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap_or_else(|e| panic!("Could not create GraphcastAgentConfig: {e}"));

    debug!("Initializing the Graphcast Agent");
    let graphcast_agent = GraphcastAgent::new(graphcast_agent_config)
        .await
        .expect("Could not create Graphcast agent");

    // A one-off setter to load the Graphcast Agent into the global static variable
    _ = GRAPHCAST_AGENT.set(graphcast_agent);

    // A one-off setter to instantiate an empty vec before populating it with incoming messages
    _ = MESSAGES.set(Arc::new(Mutex::new(vec![])));

    let mut network_chainhead_blocks: HashMap<NetworkName, BlockPointer> = HashMap::new();
    // Helper function to reuse message sending code
    async fn send_message(
        payload: Option<RadioPayloadMessage>,
        network: NetworkName,
        block_number: u64,
    ) {
        if let Err(e) = GRAPHCAST_AGENT
            .get()
            .expect("Could not retrieve Graphcast agent")
            .send_message(
                // The identifier can be any string that suits your Radio logic
                // If it doesn't matter for your Radio logic (like in this case), you can just use a UUID or a hardcoded string
                "ping-pong-content-topic".to_string(),
                network,
                block_number,
                payload,
            )
            .await
        {
            error!("Failed to send message: {}", e)
        };
    }

    // The handler specifies what to do with incoming messages.
    // There cannot be any non-deterministic (this includes async) code inside the handler.
    // That is why we're saving the message for later processing, where we will check its content and perform some action based on it.
    let radio_handler =
        |msg: Result<GraphcastMessage<RadioPayloadMessage>, WakuHandlingError>| match msg {
            Ok(msg) => {
                MESSAGES
                    .get()
                    .expect("Could not retrieve messages")
                    .lock()
                    .expect("Could not get lock on messages")
                    .push(msg);
            }
            Err(err) => {
                error!("{err}");
            }
        };

    GRAPHCAST_AGENT
        .get()
        .expect("Could not retrieve Graphcast agent")
        .register_handler(Arc::new(AsyncMutex::new(radio_handler)))
        .expect("Could not register handler");

    let network = NetworkName::from_string("goerli");

    loop {
        let indexing_statuses =
            match get_indexing_statuses(config.graph_node_endpoint.clone()).await {
                Ok(res) => res,
                Err(e) => {
                    error!("Could not query indexing statuses, pull again later: {e}");
                    sleep(Duration::from_secs(5));
                    continue;
                }
            };
        update_network_chainheads(indexing_statuses, &mut network_chainhead_blocks);
        let block_number = network_chainhead_blocks
            .entry(network)
            .or_insert(BlockPointer {
                number: 0,
                hash: "temp".to_string(),
            })
            .number;
        info!("🔗 Block number: {}", block_number);

        if block_number & 2 == 0 {
            // If block number is even, send ping message
            let msg = RadioPayloadMessage::new(
                "table".to_string(),
                std::env::args().nth(1).unwrap_or("Ping".to_string()),
            );
            send_message(Some(msg), network, block_number).await;
        } else {
            // If block number is odd, process received messages
            let messages = AsyncMutex::new(
                MESSAGES
                    .get()
                    .expect("Could not retrieve messages")
                    .lock()
                    .expect("Could not get lock on messages"),
            );
            for msg in messages.lock().await.iter() {
                let payload = msg
                    .payload
                    .as_ref()
                    .expect("Could not get radio payload payload");
                if *payload.content == *"Ping" {
                    let replay_msg =
                        RadioPayloadMessage::new("table".to_string(), "Pong".to_string());
                    send_message(Some(replay_msg), network, block_number).await;
                };
            }

            // Clear message store after processing
            messages.lock().await.clear();
        }

        // Wait before next block check
        sleep(Duration::from_secs(5));
    }
}
