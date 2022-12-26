# Graphcast SDK

## 📯 Introduction

The key requirement for an Indexer to earn indexing rewards is to submit a valid Proof of Indexing promptly. The importance of valid POIs causes many Indexers to alert each other on subgraph health in community discussions. To alleviate the Indexer workload, this Radio can aggregate and exchange POI along with a list of Indexer on-chain identities that can be used to trace reputations. With the pubsub pattern, the Indexer can effectively automatically close an allocation when some trusted Indexer(s) publishes a different POI or alert subgraph syncing failure.

To see the full idea behind Graphcast, you can check out the [GRC](https://forum.thegraph.com/t/grc-001-graphcast-a-gossip-network-for-indexers/3544/8) for it.

## 📝 Features

- Showcases the Graphcast SDK with the help of a real-world example - a POI cross-checker
- Serves as a full demo of the most critical pieces of the Graphcast SDK

## 🏃 Quickstart

📝 **As prerequisites to running the POI cross-checker, make sure that**:

1. You have registered a Graphcast operator address. You can connect a operator address to your indexer address (with a 1:1 relationship) using our very own [Registry contract](https://goerli.etherscan.io/address/0x1e408c2cf66fd3afcea0f49dc44c9f4db5575e79) (on Goerli).
2. You have a running **graph-node** instance with at least 1 fully synced subgraph.
3. You have exported the following environment variables - `ETH_NODE` and `PRIVATE_KEY`(for the operator address).
4. Have [Rust](https://www.rust-lang.org/tools/install) installed globally.

🚀 **To run the Graphcast SDK, along with the POI cross-checker Radio, run the following commands in order in two different terminal windows**:

```
cargo run --example poi-crosschecker boot
```

```
cargo run --example poi-crosschecker
```

## 🎚️ Configuring

Currently the only way to change the base configuration of the Graphcast SDK is to change your environment variables.

## 🆕 Upgrading

Updates to the SDK will be merged into the `main` branch once their respective PR has been approved. The SDK will soon be published on [crates.io](https://crates.io/).

## 🧪 Testing

There are unit tests both for the SDK and for the Radio. We recommend using [nextest](https://nexte.st/) as your test runner. Once you have it installed you can run the tests using the following commands:

For the POI cross-checker Radio tests:

```
cargo nextest run --example poi-crosschecker
```

For the SDK tests:

```
cargo nextest run
```

## 🛠️ How it works

The SDK is essentially a base layer that Radio developers can use to build their applications without needing to worry about starting everything scratch. The components that are included in the SDK are:

- Connecting to the Graphcast network, e.g., a cluster of [Waku](https://waku.org/) nodes. It also provides an interface to subscribe to receive messages on specific topics and to broadcast messages onto the network.
- Interactions with an Ethereum node.

There is also a POI cross-checker Radio in the `examples/` folder that leverages the base layer and defines the specific logic around constructing and sending messages, as well as receiving and handling them.

#### 🔃 Workflow

When an Indexer runs the POI cross-checker, they immediately start listening for new blocks on Ethereum. On a certain interval the Radio fetches all the allocations of that Indexer and saves a list of the IPFS hashes of the subgraphs that the Indexer is allocating to. Right after that we loop through the list and send a request for a normalised POI for each subgraph (using the metadata of the block that we're on) and save those POIs in an in-memopry map, below we will refer to these POIs as _local_ POIs since they are the ones that we've generated.

At the same time, other Indexers running the Radio will start doing the same, which means that messages start propagating through the network. We handle each message and add the POI from it in another in-memory map, we can refer to these POIs as _remote_ POIs since these are the ones that we've received from other network participants. The messages don't come only with the POI and subgraph hash, they also include a nonce (UNIX timestamp), block number and signature. The signature is then used to derive the sender's on-chain Indexer address. It's important to note that before saving an entry to the map, we send a request for the sender's on-chain stake, which will be used later for sorting the entries.

After another interval we compare our _local_ POIs with the _remote_ ones. We sort the remote ones so that for each subgraph (on each block) we can take the POI that is backed by the most on-chain stake (❗ This does not mean the one that is sent by the Indexer with the highest stake, but rather the one that has the most **combined** stake of all the Indexers that attested to it). After we have that top POI, we compare it with our _local_ POI for that subgraph at that block. Voilà! We now know whether our POI matches with the current consensus on the network.

## Contributing

We welcome and appreciate your contributions! Please see the [Contributor Guide](/CONTRIBUTING.md), [Code Of Conduct](/CODE_OF_CONDUCT.md) and [Security Notes](/SECURITY.md) for this repository.
