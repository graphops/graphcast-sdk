use chrono::Utc;
// Load environment variables from .env file
use dotenv::dotenv;
use std::{
    process,
    sync::atomic::{AtomicBool, Ordering},
};
use tokio::signal;

// Import Arc and Mutex for thread-safe sharing of data across threads
use std::sync::{mpsc, Arc, Mutex};
// Import Graphcast SDK types and functions for agent configuration, message handling, and more
use graphcast_sdk::{
    graphcast_agent::{message_typing::GraphcastMessage, GraphcastAgent, GraphcastAgentConfig},
    WakuMessage,
};

// Import sleep and Duration for handling time intervals and thread delays
use std::{thread::sleep, time::Duration};

// Import AsyncMutex for asynchronous mutual exclusion of shared resources
use tokio::sync::Mutex as AsyncMutex;

// Import tracing macros for logging and diagnostic purposes
use tracing::{debug, error, info, trace};

// Import SimpleMessage from the crate's types module
use types::SimpleMessage;

// Import Config from the crate's config module
use config::Config;

use crate::types::MESSAGES;

// Include the local config and types modules
mod config;
mod types;

#[tokio::main]
async fn main() {
    // This can be any string
    let radio_name = "ping-pong".to_string();
    // Loads the environment variables from .env
    dotenv().ok();

    let running = Arc::new(AtomicBool::new(true));
    let listen_running = running.clone();

    tokio::spawn(async move {
        let ctrl_c = async {
            signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let sigterm = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let sigterm = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {
                info!("Ctrl+C received! Shutting down...");
            }
            _ = sigterm => {
                info!("SIGTERM received! Shutting down...");
            }
        }
        // Set running boolean to false
        debug!("Finish the current running processes...");
        listen_running.store(false, Ordering::SeqCst);

        sleep(Duration::from_secs(5));
        debug!("Allowed 5 seconds for graceful shutdown, force exit");
        process::exit(1);
    });

    // Instantiates the configuration struct based on provided environment variables or CLI args
    let config = Config::args();
    let _parent_span = tracing::info_span!("main").entered();

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
        config.id_validation.clone(),
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

    let (sender, receiver) = mpsc::channel::<WakuMessage>();
    debug!("Initializing the Graphcast Agent");
    let graphcast_agent = GraphcastAgent::new(graphcast_agent_config, sender.clone())
        .await
        .expect("Could not create Graphcast agent");
    // Original sender not used, simply drop it
    drop(sender);

    // A one-off setter to instantiate an empty vec before populating it with incoming messages
    _ = MESSAGES.set(Arc::new(Mutex::new(vec![])));

    // The handler specifies what to do with incoming messages.
    // This is where you can define multiple message types and how they gets handled by the radio
    // by chaining radio payload typed decode and handler functions
    let receiver_running = running.clone();
    let receiver_handler = tokio::spawn(async move {
        while receiver_running.load(Ordering::SeqCst) {
            match receiver.recv() {
                Ok(msg) => {
                    trace!(
                        "Radio operator received a Waku message from Graphcast agent, now try to fit it to Graphcast Message with Radio specified payload"
                    );
                    let _ = GraphcastMessage::<SimpleMessage>::decode(msg.payload())
                        .map(|msg| {
                            msg.payload.radio_handler();
                        })
                        .map_err(|err| {
                            error!(
                                error = tracing::field::debug(&err),
                                "Failed to handle Waku signal"
                            );
                            err
                        });
                }
                Err(e) => {
                    trace!(e = e.to_string(), "All senders have been dropped, exiting");
                    break;
                }
            }
        }
    });

    // Main loop of the application
    _ = main_loop(&graphcast_agent, running).await;

    match graphcast_agent.stop() {
        Ok(_) => {
            debug!("Graphcast agent successful shutdown");
            receiver_handler.await.unwrap();
            debug!("Operator message receiver successful shutdown");
        }
        Err(e) => panic!("Cannot shutdown Graphcast agent: {:#?}", e),
    }

    debug!("Exiting the program");
}

/// Main event loop to send ping and respond pong
async fn main_loop(agent: &GraphcastAgent, running: Arc<AtomicBool>) {
    let mut block_number = 0;
    while running.load(Ordering::SeqCst) {
        block_number += 1;
        info!(block = block_number, "🔗 Block number");
        if block_number & 2 == 0 {
            // If block number is even, send ping message
            let msg = SimpleMessage::new(
                "table".to_string(),
                std::env::args().nth(1).unwrap_or("Ping".to_string()),
            );
            if let Err(e) = agent
                .send_message(
                    // The identifier can be any string that suits your Radio logic
                    // If it doesn't matter for your Radio logic (like in this case), you can just use a UUID or a hardcoded string
                    "ping-pong-content-topic",
                    msg,
                    Utc::now().timestamp(),
                )
                .await
            {
                error!(error = tracing::field::debug(&e), "Failed to send message");
            };
            // agent.send_message(msg).await;
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
                if msg.content == *"Ping" {
                    let replay_msg = SimpleMessage::new("table".to_string(), "Pong".to_string());
                    // send_message(replay_msg).await;
                    if let Err(e) = agent
                        .send_message(
                            "ping-pong-content-topic",
                            replay_msg,
                            Utc::now().timestamp(),
                        )
                        .await
                    {
                        error!(error = tracing::field::debug(&e), "Failed to send message");
                    };
                };
            }

            // Clear message store after processing
            messages.lock().await.clear();
        }

        // Wait before next block check
        sleep(Duration::from_secs(5));
    }
}
