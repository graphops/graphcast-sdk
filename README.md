# Graphcast SDK

[![Docs](https://img.shields.io/badge/docs-latest-brightgreen.svg)](https://docs.graphops.xyz/graphcast/intro)

## Introduction

Graphcast is a decentralized, distributed peer-to-peer (P2P) communication tool that allows Indexers across the network to exchange information in real-time. Today, network participants coordinate with one another using the protocol by submitting on-chain transactions that update the shared global state in The Graph Network. These transactions cost gas, which makes some types of signaling or coordination between participants too expensive. In other words, sending signals to other network participants has a high cost that is determined by the cost of transacting on the Ethereum blockchain. Graphcast solves this problem.

To see the full idea behind Graphcast, you can check out the [GRC](https://forum.thegraph.com/t/grc-001-graphcast-a-gossip-network-for-indexers/3544/8) for it.

## Upgrading

Updates to the SDK will be merged into the `main` branch once their respective PR has been approved. The SDK will soon be published on [crates.io](https://crates.io/).

## Testing

We recommend using [nextest](https://nexte.st/) as your test runner. Once you have it installed you can run the the suite using the following command:

```
cargo nextest run
```

## How does the Graphcast SDK work?

The SDK is essentially a base layer that Radio developers can use to build their applications without needing to worry about starting everything scratch. The components that are included in the SDK are:

- Connecting to the Graphcast network, e.g., a cluster of [Waku](https://waku.org/) nodes. It also provides an interface to subscribe to receive messages on specific topics and to broadcast messages onto the network.
- Interactions with an Ethereum node.

There is also a POI cross-checker Radio in the `examples/` folder that leverages the base layer and defines the specific logic around constructing and sending messages, as well as receiving and handling them.

## Contributing

We welcome and appreciate your contributions! Please see the [Contributor Guide](/CONTRIBUTING.md), [Code Of Conduct](/CODE_OF_CONDUCT.md) and [Security Notes](/SECURITY.md) for this repository.
