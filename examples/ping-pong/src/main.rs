use dotenv::dotenv;
use ethers::{
    providers::{Http, Middleware, Provider},
    types::U64,
};
use graphcast_sdk::{
    graphcast_agent::{message_typing::GraphcastMessage, GraphcastAgent},
    init_tracing, read_boot_node_addresses, NetworkName,
};
use once_cell::sync::OnceCell;
use std::env;
use std::sync::{Arc, Mutex};
use std::{thread::sleep, time::Duration};
use tokio::sync::Mutex as AsyncMutex;
use tracing::{debug, error, info, warn};
use types::RadioPayloadMessage;

mod types;

#[tokio::main]
async fn main() {
    // Loads the environment variables from our .env file
    dotenv().ok();

    // Enables tracing, you can set your preferred log level in your .env file
    // You can choose one of: TRACE, DEBUG, INFO, WARN, ERROR
    // If none is provided, defaults to INFO
    init_tracing().expect("Could not set up global default subscriber");

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

    let waku_host = env::var("WAKU_HOST").ok();
    let waku_port = env::var("WAKU_PORT").ok();
    let waku_node_key = env::var("WAKU_NODE_KEY").ok();
    // The private key for you GraphcastID's address
    let private_key = env::var("PRIVATE_KEY").expect(
        "No GraphcastID's private key provided. Please specify a PRIVATE_KEY environment variable.",
    );

    // An Ethereum node url in order to read on-chain data using a provider
    let eth_node = env::var("ETH_NODE")
        .expect("No ETH URL provided. Please specify an ETH_NODE environment variable.");
    // Graph node status endpoint
    let graph_node_endpoint = env::var("GRAPH_NODE_STATUS_ENDPOINT")
        .expect("No GRAPH_NODE_STATUS_ENDPOINT provided. Please specify an endpoint.");
    // Define the pubsub namespace of Graphcast network
    let graphcast_network = env::var("GRAPHCAST_NETWORK")
        .expect("No GRAPHCAST_NETWORK provided. Please specify 1 for mainnet, 5 for goerli.");

    // Provider is only used to generate block number for Ping-pong
    let provider: Provider<Http> =
        Provider::<Http>::try_from(eth_node.clone()).expect("Could not create Ethereum provider");

    // This can be any string
    let radio_name: &str = "ping-pong";

    /// A constant defining the goerli registry subgraph endpoint.
    pub const REGISTRY_SUBGRAPH: &str =
        "https://api.thegraph.com/subgraphs/name/hopeyen/gossip-registry-test";

    /// A constant defining the goerli network subgraph endpoint.
    pub const NETWORK_SUBGRAPH: &str = "https://gateway.testnet.thegraph.com/network";
    // subtopics are optionally provided and used as the content topic identifier of the message subject,
    // if not provided then they are generated based on indexer allocations
    let subtopics = vec!["ping-pong-content-topic".to_string()];

    let boot_node_addresses = read_boot_node_addresses();

    let graphcast_agent = GraphcastAgent::new(
        // private_key resolves into ethereum wallet and indexer identity.
        private_key,
        // radio_name is used as part of the content topic for the radio application
        radio_name,
        REGISTRY_SUBGRAPH,
        NETWORK_SUBGRAPH,
        &graph_node_endpoint,
        boot_node_addresses,
        Some(&graphcast_network),
        subtopics,
        // Waku node address is set up by optionally providing a host and port, and an advertised address to be connected among the waku peers
        // Advertised address can be any multiaddress that is self-describing and support addresses for any network protocol (tcp, udp, ip; tcp6, udp6, ip6 for IPv6)
        waku_node_key,
        waku_host,
        waku_port,
        None,
    )
    .await
    .expect("Could not create Graphcast agent");

    // A one-off setter to load the Graphcast Agent into the global static variable
    _ = GRAPHCAST_AGENT.set(graphcast_agent);

    // A one-off setter to instantiate an empty vec before populating it with incoming messages
    _ = MESSAGES.set(Arc::new(Mutex::new(vec![])));

    // Helper function to reuse message sending code
    async fn send_message(
        payload: Option<RadioPayloadMessage>,
        network: NetworkName,
        block_number: u64,
    ) {
        match GRAPHCAST_AGENT
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
                MESSAGES
                    .get()
                    .expect("Could not retrieve messages")
                    .lock()
                    .expect("Could not get lock on messages")
                    .push(msg);
            }
            Err(err) => {
                println!("{err}");
            }
        };

    GRAPHCAST_AGENT
        .get()
        .expect("Could not retrieve Graphcast agent")
        .register_handler(Arc::new(AsyncMutex::new(radio_handler)))
        .expect("Could not register handler");

    // Limit Ping-pong radio for testing purposes, update after nwaku nodes update their namespace
    let network = if *"1" == graphcast_network {
        NetworkName::from_string("mainnet")
    } else {
        NetworkName::from_string("goerli")
    };

    loop {
        let block_number = match provider.get_block_number().await {
            Ok(num) => U64::as_u64(&num),
            Err(e) => {
                warn!("Could not get block number from provider. Context: {e}");
                sleep(Duration::from_secs(1));
                continue;
            }
        };
        debug!("🔗 Block number: {}", block_number);

        if block_number & 2 == 0 {
            // If block number is even, send ping message
            let msg = RadioPayloadMessage::new("table".to_string(), "Ping222".to_string());
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
