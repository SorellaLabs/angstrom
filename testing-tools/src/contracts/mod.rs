use alloy::{
    contract::RawCallBuilder,
    primitives::{address, keccak256, Address, Bytes, B256, U160, U256},
    sol_types::SolValue
};
use angstrom_types::contract_bindings::mockrewardsmanager::MockRewardsManager;

const CREATE2_FACTORY: Address = address!("4e59b44847b379578588920cA78FbF26c0B4956C");

/// Attempt to find a target address that includes the appropriate flags
/// Returns the address found and the salt needed to pad the initcode to
/// deploy to that address
pub fn mine_address(flags: U160, mask: U160, initcode: &Bytes) -> (Address, U256) {
    mine_address_with_factory(None, flags, mask, initcode)
}

pub fn mine_address_with_factory(
    factory: Option<Address>,
    flags: U160,
    mask: U160,
    initcode: &Bytes
) -> (Address, U256) {
    let init_code_hash = keccak256(initcode);
    let mut salt = U256::ZERO;
    let create2_factory = factory.unwrap_or(CREATE2_FACTORY);
    let mut counter: u128 = 0;
    loop {
        let target_address: Address = create2_factory.create2(B256::from(salt), init_code_hash);
        let u_address: U160 = target_address.into();
        if (u_address & mask) == flags {
            break;
        }
        salt += U256::from(1_u8);
        counter += 1;
        if counter > 100_000 {
            panic!("We tried this too many times!")
        }
    }
    let final_address = create2_factory.create2(B256::from(salt), init_code_hash);
    (final_address, salt)
}

pub async fn deploy_mock_rewards_manager<
    T: alloy::contract::private::Transport + ::core::clone::Clone,
    P: alloy::contract::private::Provider<T, N>,
    N: alloy::contract::private::Network
>(
    provider: &P,
    pool_manager: Address
) -> Address
where {
    // Setup our flags and mask
    // Flags for our MockRewardsManager address
    let before_swap = U160::from(1_u8) << 7;
    let before_initialize = U160::from(1_u8) << 13;
    let before_add_liquidity = U160::from(1_u8) << 11;
    let after_remove_liquidity = U160::from(1_u8) << 8;

    let flags = before_swap | before_initialize | before_add_liquidity | after_remove_liquidity;
    let mask: U160 = (U160::from(1_u8) << 14) - U160::from(1_u8);

    let mock_builder = MockRewardsManager::deploy_builder(&provider, pool_manager);
    let (mock_tob, salt) = mine_address(flags, mask, mock_builder.calldata());
    let final_mock_initcode = [salt.abi_encode(), mock_builder.calldata().to_vec()].concat();
    let raw_deploy = RawCallBuilder::new_raw_deploy(&provider, final_mock_initcode.into());
    raw_deploy.call_raw().await.unwrap();
    mock_tob
}
