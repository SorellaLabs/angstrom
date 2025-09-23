mod contract_bindings;
pub use contract_bindings::*;

pub type PoolId = alloy_primitives::FixedBytes<32>;

mod _private {
    use alloy_primitives::keccak256;
    use alloy_sol_types::SolValue;

    use super::{
        PoolId, angstrom::Angstrom, pool_manager::PoolManager, position_manager::PositionManager
    };

    macro_rules! pool_key_to_id {
        ($contract:ident) => {
            impl From<$contract::PoolKey> for PoolId {
                fn from(value: $contract::PoolKey) -> Self {
                    keccak256(value.abi_encode())
                }
            }

            impl From<&$contract::PoolKey> for PoolId {
                fn from(value: &$contract::PoolKey) -> Self {
                    keccak256(value.abi_encode())
                }
            }

            impl Copy for $contract::PoolKey {}
        };
    }

    pool_key_to_id!(PoolManager);
    pool_key_to_id!(PositionManager);
    pool_key_to_id!(Angstrom);
}
