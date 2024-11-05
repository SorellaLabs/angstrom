use alloy::{contract::RawCallBuilder, primitives::Address, sol_types::SolValue};
use angstrom_types::contract_bindings::angstrom::Angstrom;

use super::{mine_address, uniswap_flags::UniswapFlags, DEFAULT_CREATE2_FACTORY};

pub async fn deploy_angstrom<
    T: alloy::contract::private::Transport + ::core::clone::Clone,
    P: alloy::contract::private::Provider<T, N>,
    N: alloy::contract::private::Network
>(
    provider: &P,
    pool_manager: Address,
    controller: Address,
    feemaster: Address
) -> Address {
    deploy_angstrom_with_factory(
        provider,
        pool_manager,
        DEFAULT_CREATE2_FACTORY,
        controller,
        feemaster
    )
    .await
}

pub async fn deploy_angstrom_with_factory<
    T: alloy::contract::private::Transport + ::core::clone::Clone,
    P: alloy::contract::private::Provider<T, N>,
    N: alloy::contract::private::Network
>(
    provider: &P,
    pool_manager: Address,
    factory: Address,
    controller: Address,
    feemaster: Address
) -> Address {
    // Setup our flags and mask
    let flags = UniswapFlags::BeforeSwap
        | UniswapFlags::BeforeInitialize
        | UniswapFlags::BeforeAddLiquidity
        | UniswapFlags::BeforeRemoveLiquidity;

    let mock_builder = Angstrom::deploy_builder(&provider, pool_manager, controller, feemaster);
    let (mock_tob_address, salt) =
        mine_address(flags, UniswapFlags::mask(), mock_builder.calldata());
    let final_mock_initcode = [salt.abi_encode(), mock_builder.calldata().to_vec()].concat();
    RawCallBuilder::new_raw(&provider, final_mock_initcode.into())
        .to(factory)
        .gas(10_000_000_u64)
        .send()
        .await
        .unwrap()
        .watch()
        .await
        .unwrap();
    mock_tob_address
}
