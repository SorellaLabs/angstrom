#[cfg(feature = "testnet-sepolia")]
pub const DEFAULT_RPC: [&str; 4] = [
    "https://ethereum-sepolia.rpc.subquery.network/public",
    "https://endpoints.omniatech.io/v1/eth/sepolia/public",
    "https://sepolia.gateway.tenderly.co",
    "https://1rpc.io/sepolia"
];

#[cfg(not(feature = "testnet-sepolia"))]
pub const DEFAULT_RPC: [&str; 4] = [
    "https://eth.llamarpc.com",
    "https://eth.drpc.org",
    "https://mainnet.gateway.tenderly.co",
    "https://ethereum-rpc.publicnode.com"
];

#[cfg(feature = "testnet-sepolia")]
pub const MEV_RPC: [&str; 1] = ["https://relay-sepolia.flashbots.net"];

#[cfg(not(feature = "testnet-sepolia"))]
pub const MEV_RPC: [&str; 1] = ["https://relay.flashbots.net"];

#[cfg(feature = "testnet-sepolia")]
pub const ANGSTROM_RPC: [&str; 1] = ["https://relay-sepolia.flashbots.net"];

#[cfg(not(feature = "testnet-sepolia"))]
pub const ANGSTROM_RPC: [&str; 1] = ["https://rpc.titanbuilder.xyz"];
