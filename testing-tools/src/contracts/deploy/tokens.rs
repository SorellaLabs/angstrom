use alloy::primitives::Address;
use angstrom_types::contract_bindings::mintable_mock_erc_20::MintableMockERC20;

pub async fn mint_token_pair<T, N, P>(provider: &P) -> (Address, Address)
where
    T: Clone + Send + Sync + alloy::transports::Transport,
    N: alloy::providers::Network,
    P: alloy::providers::Provider<T, N>
{
    let first_token = MintableMockERC20::deploy(provider).await.unwrap();
    let second_token = MintableMockERC20::deploy(provider).await.unwrap();
    if first_token.address() < second_token.address() {
        (*first_token.address(), *second_token.address())
    } else {
        (*second_token.address(), *first_token.address())
    }
}
