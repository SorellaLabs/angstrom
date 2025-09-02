mod devnet;
mod node;
mod op_testnet;
mod replay;
mod testnet;

pub use devnet::*;
pub use node::*;
pub use op_testnet::*;
pub use replay::*;
pub use testnet::*;

#[derive(Debug, Clone)]
pub enum TestingConfigKind {
    Testnet,
    Devnet,
    Replay
}
