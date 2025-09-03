mod devnet;
mod node;
mod testnet;

pub use devnet::*;
pub use node::*;
pub use testnet::*;

#[derive(Debug, Clone)]
pub enum TestingConfigKind {
    Testnet,
    Devnet,
    Replay
}
