use dotenv::dotenv;
use ethers::{
    providers::{Http, Middleware, Provider},
    types::U64,
};
use graphcast_sdk::gossip_agent::message_typing::GraphcastMessage;
use graphcast_sdk::gossip_agent::GossipAgent;
use once_cell::sync::OnceCell;
use std::env;
use std::sync::{Arc, Mutex};
use std::{thread::sleep, time::Duration};
use tokio::sync::Mutex as AsyncMutex;

#[tokio::main]
async fn main() {
    // Loads the environment variables from our .env file
    dotenv().ok();

    /// A global static (singleton) instance of A GraphcastMessage vector.
    /// It is used to save incoming messages after they've been validated, in order
    /// defer their processing for later, because async code is required for the processing but
    /// it is not allowed in the handler itself.
    pub static MESSAGES: OnceCell<Arc<Mutex<Vec<GraphcastMessage>>>> = OnceCell::new();

    /// The Gossip Agent instance must be a global static variable (for the time being).
    /// This is because the Radio handler requires a static immutable context and
    /// the handler itself is being passed into the Gossip Agent, so it needs to be static as well.
    pub static GOSSIP_AGENT: OnceCell<GossipAgent> = OnceCell::new();

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
        private_key,
        eth_node,
        radio_name,
        REGISTRY_SUBGRAPH,
        NETWORK_SUBGRAPH,
        // Content topics that the Radio subscribes to
        // If we pass in `None` Graphcast will default to using the ipfs hashes of the subgraphs that the Indexer is allocating to.
        // But in this case we will override it with something much more simple.
        Some(vec!["ping-pong-content-topic"]),
        None,
        None,
    )
    .await
    .unwrap();

    // A one-off setter to load the Gossip Agent into the global static variable
    _ = GOSSIP_AGENT.set(gossip_agent);

    // A one-off setter to instantiate an empty vec before populating it with incoming messages
    _ = MESSAGES.set(Arc::new(Mutex::new(vec![])));

    // Helper function to reuse message sending code
    async fn send_message(content: String, block_number: u64) {
        match GOSSIP_AGENT
            .get()
            .unwrap()
            .send_message(
                // The identifier can be any string that suits your Radio logic
                // If it doesn't matter for your Radio logic (like in this case), you can just use a UUID or a hardcoded string
                "ping-pong-content-topic".to_string(),
                block_number,
                content,
            )
            .await
        {
            Ok(sent) => println!("Sent message id:: {}", sent),
            Err(e) => println!("Failed to send message: {}", e),
        };
    }

    // The handler specifies what to do with incoming messages.
    // There cannot be any non-deterministic (this includes async) code inside the handler.
    // That is why we're saving the message for later processing, where we will check it's content and perform some action based on it.
    let radio_handler = |msg: Result<GraphcastMessage, anyhow::Error>| match msg {
        Ok(msg) => {
            println!("New message received! {:?}\n Saving to message store.", msg);
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
        println!("🔗 Block number: {}", block_number);

        if block_number & 2 == 0 {
            // If block number is even, send ping message
            send_message("Ping".to_string(), block_number).await;
        } else {
            // If block number is odd, process received messages
            let messages = AsyncMutex::new(MESSAGES.get().unwrap().lock().unwrap());
            for msg in messages.lock().await.iter() {
                if msg.content == *"Ping" {
                    send_message("Pong".to_string(), block_number).await;
                }
            }

            // Clear message store after processing
            messages.lock().await.clear();
        }

        // Wait before next block check
        sleep(Duration::from_secs(5));
    }
}
