use chrono::Utc;
// Load environment variables from .env file
use dotenv::dotenv;

// Import Graphcast SDK types and functions for agent configuration, message handling, and more
use graphcast_sdk::graphcast_agent::{
    message_typing::GraphcastMessage, waku_handling::WakuHandlingError, GraphcastAgent,
    GraphcastAgentConfig,
};

// Import the OnceCell container for lazy initialization of global/static data
use once_cell::sync::OnceCell;

// Import HashMap for key-value storage

// Import Arc and Mutex for thread-safe sharing of data across threads
use std::sync::{Arc, Mutex};

// Import sleep and Duration for handling time intervals and thread delays
use std::{thread::sleep, time::Duration};

// Import AsyncMutex for asynchronous mutual exclusion of shared resources
use tokio::sync::Mutex as AsyncMutex;

// Import tracing macros for logging and diagnostic purposes
use tracing::{debug, error, info};

// Import SimpleMessage from the crate's types module
use types::SimpleMessage;

// Import Config from the crate's config module
use config::Config;

// Include the local config and types modules
mod config;
mod types;

#[tokio::main]
async fn main() {
    // This can be any string
    let radio_name = "ping-pong".to_string();
    // Loads the environment variables from .env
    dotenv().ok();

    // Instantiates the configuration struct based on provided environment variables or CLI args
    let config = Config::args();
    let _parent_span = tracing::info_span!("main").entered();

    /// A global static (singleton) instance of A GraphcastMessage vector.
    /// It is used to save incoming messages after they've been validated, in order
    /// defer their processing for later, because async code is required for the processing but
    /// it is not allowed in the handler itself.
    pub static MESSAGES: OnceCell<Arc<Mutex<Vec<GraphcastMessage<SimpleMessage>>>>> =
        OnceCell::new();

    /// The Graphcast Agent instance must be a global static variable (for the time being).
    /// This is because the Radio handler requires a static immutable context and
    /// the handler itself is being passed into the Graphcast Agent, so it needs to be static as well.
    pub static GRAPHCAST_AGENT: OnceCell<GraphcastAgent> = OnceCell::new();

    // subtopics are optionally provided and used as the content topic identifier of the message subject,
    // if not provided then they are usually generated based on indexer allocations
    let subtopics: Vec<String> = vec!["ping-pong-content-topic".to_string()];

    // GraphcastAgentConfig defines the configuration that the SDK expects from all Radios, regardless of their specific functionality
    let graphcast_agent_config = GraphcastAgentConfig::new(
        config.private_key.expect("No private key provided"),
        config.indexer_address,
        radio_name,
        config.registry_subgraph,
        config.network_subgraph,
        config.id_validation.unwrap_or_default(),
        config.graph_node_endpoint,
        None,
        Some("testnet".to_string()),
        Some(subtopics),
        None,
        None,
        None,
        None,
        Some(true),
        // Example ENR address
        Some(vec![String::from("enr:-JK4QBcfVXu2YDeSKdjF2xE5EDM5f5E_1Akpkv_yw_byn1adESxDXVLVjapjDvS_ujx6MgWDu9hqO_Az_CbKLJ8azbMBgmlkgnY0gmlwhAVOUWOJc2VjcDI1NmsxoQOUZIqKLk5xkiH0RAFaMGrziGeGxypJ03kOod1-7Pum3oN0Y3CCfJyDdWRwgiMohXdha3UyDQ")]),
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
    // Helper function to reuse message sending code
    async fn send_message(payload: SimpleMessage) {
        if let Err(e) = GRAPHCAST_AGENT
            .get()
            .expect("Could not retrieve Graphcast agent")
            .send_message(
                // The identifier can be any string that suits your Radio logic
                // If it doesn't matter for your Radio logic (like in this case), you can just use a UUID or a hardcoded string
                "ping-pong-content-topic",
                payload,
                Utc::now().timestamp(),
            )
            .await
        {
            error!(error = tracing::field::debug(&e), "Failed to send message");
        };
    }

    // The handler specifies what to do with incoming messages.
    // There cannot be any non-deterministic (this includes async) code inside the handler.
    // That is why we're saving the message for later processing, where we will check its content and perform some action based on it.
    let radio_handler = |msg: Result<GraphcastMessage<SimpleMessage>, WakuHandlingError>| match msg
    {
        Ok(msg) => {
            MESSAGES
                .get()
                .expect("Could not retrieve messages")
                .lock()
                .expect("Could not get lock on messages")
                .push(msg);
        }
        Err(err) => {
            error!(
                error = tracing::field::debug(&err),
                "Failed to handle Waku signal"
            );
        }
    };

    GRAPHCAST_AGENT
        .get()
        .expect("Could not retrieve Graphcast agent")
        .register_handler(Arc::new(AsyncMutex::new(radio_handler)))
        .expect("Could not register handler");

    let mut block_number = 0;

    loop {
        block_number += 1;
        info!(block = block_number, "🔗 Block number");
        if block_number & 2 == 0 {
            // If block number is even, send ping message
            let msg = SimpleMessage::new(
                "table".to_string(),
                std::env::args().nth(1).unwrap_or("Ping".to_string()),
            );
            send_message(msg).await;
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
                if msg.payload.content == *"Ping" {
                    let replay_msg = SimpleMessage::new("table".to_string(), "Pong".to_string());
                    send_message(replay_msg).await;
                };
            }

            // Clear message store after processing
            messages.lock().await.clear();
        }

        // Wait before next block check
        sleep(Duration::from_secs(5));
    }
}
