use once_cell::sync::Lazy;
use std::fmt;

/// Struct for Network and block interval for updates
#[derive(Debug, Clone)]
pub struct Network {
    pub name: NetworkName,
    pub interval: u64,
}

/// List of supported networks
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum NetworkName {
    Goerli,
    Mainnet,
    Gnosis,
    Hardhat,
    ArbitrumOne,
    ArbitrumGoerli,
    Avalanche,
    Polygon,
    Celo,
    Optimism,
    Fantom,
    Unknown,
}

impl NetworkName {
    pub fn from_string(name: &str) -> Self {
        match name {
            "goerli" => NetworkName::Goerli,
            "mainnet" => NetworkName::Mainnet,
            "gnosis" => NetworkName::Gnosis,
            "hardhat" => NetworkName::Hardhat,
            "arbitrum-one" => NetworkName::ArbitrumOne,
            "arbitrum-goerli" => NetworkName::ArbitrumGoerli,
            "avalanche" => NetworkName::Avalanche,
            "polygon" => NetworkName::Polygon,
            "celo" => NetworkName::Celo,
            "optimism" => NetworkName::Optimism,
            "fantom" => NetworkName::Fantom,
            _ => NetworkName::Unknown,
        }
    }
}

impl fmt::Display for NetworkName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            NetworkName::Goerli => "goerli",
            NetworkName::Mainnet => "mainnet",
            NetworkName::Gnosis => "gnosis",
            NetworkName::Hardhat => "hardhat",
            NetworkName::ArbitrumOne => "arbitrum-one",
            NetworkName::ArbitrumGoerli => "arbitrum-goerli",
            NetworkName::Avalanche => "avalanche",
            NetworkName::Polygon => "polygon",
            NetworkName::Celo => "celo",
            NetworkName::Optimism => "optimism",
            NetworkName::Fantom => "fantom",
            NetworkName::Unknown => "unknown",
        };

        write!(f, "{name}")
    }
}

/// Maintained static list of supported Networks, the intervals target ~5minutes
/// depending on the blockchain average block processing time
pub static NETWORKS: Lazy<Vec<Network>> = Lazy::new(|| {
    vec![
        // Goerli (Ethereum Testnet): ~15 seconds
        Network {
            name: NetworkName::from_string("goerli"),
            interval: 20,
        },
        // Mainnet (Ethereum): ~10-12 seconds
        Network {
            name: NetworkName::from_string("mainnet"),
            interval: 30,
        },
        // Gnosis: ~5 seconds
        Network {
            name: NetworkName::from_string("gnosis"),
            interval: 60,
        },
        // Local test network
        Network {
            name: NetworkName::from_string("hardhat"),
            interval: 10,
        },
        // ArbitrumOne: ~0.25-1 second
        Network {
            name: NetworkName::from_string("arbitrum-one"),
            interval: 600,
        },
        // ArbitrumGoerli (Arbitrum Testnet): ~.6 seconds
        Network {
            name: NetworkName::from_string("arbitrum-goerli"),
            interval: 500,
        },
        // Avalanche: ~3-5 seconds
        Network {
            name: NetworkName::from_string("avalanche"),
            interval: 60,
        },
        // Polygon: ~2 seconds
        Network {
            name: NetworkName::from_string("polygon"),
            interval: 150,
        },
        // Celo: ~5-10 seconds
        Network {
            name: NetworkName::from_string("celo"),
            interval: 30,
        },
        // Optimism: ~10-15 seconds
        Network {
            name: NetworkName::from_string("optimism"),
            interval: 20,
        },
        // Fantom: ~2-3 seconds
        Network {
            name: NetworkName::from_string("optimism"),
            interval: 100,
        },
    ]
});
