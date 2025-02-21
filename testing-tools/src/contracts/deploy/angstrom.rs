use alloy::{
    contract::RawCallBuilder, network::Ethereum, primitives::Address, sol_types::SolValue
};
use alloy_sol_types::SolCall;
use angstrom_types::contract_bindings::angstrom::Angstrom;

use super::{mine_create3_address, mine_create3_address_uni, SUB_ZERO_FACTORY};

pub async fn deploy_angstrom_create3<
    T: alloy::contract::private::Transport + ::core::clone::Clone,
    P: alloy::contract::private::Provider<T, Ethereum> + alloy::providers::WalletProvider<Ethereum>
>(
    provider: &P,
    pool_manager: Address,
    controller: Address
) -> eyre::Result<Address> {
    let owner = provider.default_signer_address();

    let mut code = Angstrom::BYTECODE.to_vec();
    code.append(&mut (pool_manager, controller).abi_encode().to_vec());

    let (mock_tob_address, salt, nonce) = mine_create3_address(owner);

    let mint_call = _private::mintCall { to: owner, id: salt, nonce };

    RawCallBuilder::<(), _, _>::new_raw(&provider, mint_call.abi_encode().into())
        .to(SUB_ZERO_FACTORY)
        .from(owner)
        .gas(50e6 as u64)
        .send()
        .await?
        .watch()
        .await?;

    let deploy_call = _private::deployCall { id: salt, initcode: code.into() };

    RawCallBuilder::<(), _, _>::new_raw(&provider, deploy_call.abi_encode().into())
        .from(owner)
        .gas(50e6 as u64)
        .to(SUB_ZERO_FACTORY)
        .send()
        .await?
        .watch()
        .await?;

    Ok(mock_tob_address)
}

mod _private {
    use alloy::sol;

    sol! {
        function mint(address to, uint256 id, uint8 nonce);

        function deploy(uint256 id, bytes initcode) returns (address);
    }
}

pub async fn deploy_uni_create3<
    T: alloy::contract::private::Transport + ::core::clone::Clone,
    P: alloy::contract::private::Provider<T, Ethereum> + alloy::providers::WalletProvider<Ethereum>
>(
    provider: &P,
    controller: Address
) -> Address {
    let owner = provider.default_signer_address();

    let mut code = angstrom_types::contract_bindings::pool_manager::PoolManager::BYTECODE.to_vec();
    code.append(&mut controller.abi_encode().to_vec());

    let (mock_tob_address, salt, nonce) = mine_create3_address_uni(owner);

    let mint_call = _private::mintCall { to: owner, id: salt, nonce };

    RawCallBuilder::new_raw(&provider, mint_call.abi_encode().into())
        .to(SUB_ZERO_FACTORY)
        .from(owner)
        .gas(50e6 as u64)
        .send()
        .await
        .unwrap()
        .watch()
        .await
        .unwrap();

    let deploy_call = _private::deployCall { id: salt, initcode: code.into() };

    RawCallBuilder::new_raw(&provider, deploy_call.abi_encode().into())
        .from(owner)
        .gas(50e6 as u64)
        .to(SUB_ZERO_FACTORY)
        .gas(50e6 as u64)
        .send()
        .await
        .unwrap()
        .watch()
        .await
        .unwrap();

    mock_tob_address
}
