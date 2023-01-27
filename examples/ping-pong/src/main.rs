use dotenv::dotenv;
use ethers::{
    providers::{Http, Middleware, Provider},
    types::U64,
};
use graphcast_sdk::{
    gossip_agent::{message_typing::GraphcastMessage, GossipAgent},
    init_tracing,
};
use once_cell::sync::OnceCell;
use std::env;
use std::sync::{Arc, Mutex};
use std::{thread::sleep, time::Duration};
use tokio::sync::Mutex as AsyncMutex;
use tracing::{debug, error, info};
use types::RadioPayloadMessage;

mod types;

#[tokio::main]
async fn main() {
    // Loads the environment variables from our .env file
    dotenv().ok();

    // Enables tracing, you can set your preferred log level in your .env file
    // You can choose one of: TRACE, DEBUG, INFO, WARN, ERROR
    // If none is provided, defaults to INFO
    init_tracing();

    /// A global static (singleton) instance of A GraphcastMessage vector.
    /// It is used to save incoming messages after they've been validated, in order
    /// defer their processing for later, because async code is required for the processing but
    /// it is not allowed in the handler itself.
    pub static MESSAGES: OnceCell<Arc<Mutex<Vec<GraphcastMessage<RadioPayloadMessage>>>>> =
        OnceCell::new();

    /// The Gossip Agent instance must be a global static variable (for the time being).
    /// This is because the Radio handler requires a static immutable context and
    /// the handler itself is being passed into the Gossip Agent, so it needs to be static as well.
    pub static GOSSIP_AGENT: OnceCell<GossipAgent> = OnceCell::new();

    let waku_host = env::var("WAKU_HOST").ok();
    let waku_port = env::var("WAKU_PORT").ok();
    // The private key for you Graphcast operator address
    let private_key = env::var("PRIVATE_KEY").expect("No operator private key provided.");

    // An Ethereum node url in order to read on-chain data using a provider
    let eth_node = env::var("ETH_NODE").expect("No ETH URL provided.");
    let provider: Provider<Http> = Provider::<Http>::try_from(eth_node.clone()).unwrap();

    // This can be any string
    let radio_name: &str = "ping-pong";

    /// A constant defining the goerli registry subgraph endpoint.
    pub const REGISTRY_SUBGRAPH: &str =
        "https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test";

    /// A constant defining the goerli network subgraph endpoint.
    pub const NETWORK_SUBGRAPH: &str = "https://gateway.testnet.thegraph.com/network";

    let gossip_agent = GossipAgent::new(
        // private_key resolves into ethereum wallet and indexer identity.
        private_key,
        eth_node,
        // radio_name is used as part of the content topic for the radio application
        radio_name,
        REGISTRY_SUBGRAPH,
        NETWORK_SUBGRAPH,
        // subtopic optionally provided and used as the content topic identifier of the message subject,
        // if not provided then they are generated based on indexer allocations
        Some(vec!["ping-pong-content-topic"]),
        // Waku node address is set up by optionally providing a host and port, and an advertised address to be connected among the waku peers
        // Advertised address can be any multiaddress that is self-describing and support addresses for any network protocol (tcp, udp, ip; tcp6, udp6, ip6 for IPv6)
        waku_host,
        waku_port,
        None,
    )
    .await
    .unwrap();

    // A one-off setter to load the Gossip Agent into the global static variable
    _ = GOSSIP_AGENT.set(gossip_agent);

    // A one-off setter to instantiate an empty vec before populating it with incoming messages
    _ = MESSAGES.set(Arc::new(Mutex::new(vec![])));

    // Helper function to reuse message sending code
    async fn send_message(payload: Option<RadioPayloadMessage>, block_number: u64) {
        match GOSSIP_AGENT
            .get()
            .unwrap()
            .send_message(
                // The identifier can be any string that suits your Radio logic
                // If it doesn't matter for your Radio logic (like in this case), you can just use a UUID or a hardcoded string
                "ping-pong-content-topic".to_string(),
                block_number,
                payload,
            )
            .await
        {
            Ok(sent) => info!("Sent message id: {}", sent),
            Err(e) => error!("Failed to send message: {}", e),
        };
    }

    // The handler specifies what to do with incoming messages.
    // There cannot be any non-deterministic (this includes async) code inside the handler.
    // That is why we're saving the message for later processing, where we will check its content and perform some action based on it.
    let radio_handler =
        |msg: Result<GraphcastMessage<RadioPayloadMessage>, anyhow::Error>| match msg {
            Ok(msg) => {
                MESSAGES.get().unwrap().lock().unwrap().push(msg);
            }
            Err(err) => {
                println!("{}", err);
            }
        };

    GOSSIP_AGENT
        .get()
        .unwrap()
        .register_handler(Arc::new(Mutex::new(radio_handler)));

    loop {
        let block_number = U64::as_u64(&provider.get_block_number().await.unwrap());
        debug!("🔗 Block number: {}", block_number);

        if block_number & 2 == 0 {
            let msg = RadioPayloadMessage::new("table".to_string(), "Ping".to_string());
            // If block number is even, send ping message
            send_message(Some(msg), block_number).await;
        } else {
            // If block number is odd, process received messages
            let messages = AsyncMutex::new(MESSAGES.get().unwrap().lock().unwrap());
            for msg in messages.lock().await.iter() {
                let payload = msg
                    .payload
                    .as_ref()
                    .expect("Could not get radio payload payload");
                if *payload.content == *"Ping" {
                    let replay_msg =
                        RadioPayloadMessage::new("table".to_string(), "Pong".to_string());
                    send_message(Some(replay_msg), block_number).await;
                };
            }

            // Clear message store after processing
            messages.lock().await.clear();
        }

        // Wait before next block check
        sleep(Duration::from_secs(5));
    }
}
