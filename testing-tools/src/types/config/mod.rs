mod devnet;
mod node;
mod replay;
mod testnet;

pub use devnet::*;
pub use node::*;
pub use replay::*;
pub use testnet::*;

#[derive(Debug, Clone)]
pub enum TestingConfigKind {
    Testnet,
    Devnet,
    Replay
}
