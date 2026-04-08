use std::collections::HashMap;

use alloy_primitives::Address;
use angstrom_types::primitive::PoolId;
use itertools::Itertools;
use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider, ReceiptProvider};

use crate::{
    controllers::enviroments::{AngstromTestnet, DevnetStateMachine},
    providers::WalletProvider,
    types::{StateMachineCheckHookFn, config::DevnetConfig}
};

pub trait WithCheck<C>
where
    C: BlockReader<Block = reth::primitives::block::Block>
        + ReceiptProvider<Receipt = reth::primitives::receipt::Receipt>
        + HeaderProvider<Header = reth::primitives::Header>
        + ChainSpecProvider<ChainSpec: Hardforks>
        + Unpin
        + Clone
        + 'static
{
    type FunctionOutput;

    fn check_block(&mut self, block_number: u64);

    /// checks the [TokenPriceGenerator] has certain pairs/pools
    fn check_token_price_gen_has_pools(
        &mut self,
        checked_pair_to_pool: HashMap<(Address, Address), PoolId>
    );
}

impl<C> WithCheck<C> for DevnetStateMachine<'_, C>
where
    C: BlockReader<Block = reth::primitives::block::Block>
        + ReceiptProvider<Receipt = reth::primitives::receipt::Receipt>
        + HeaderProvider<Header = reth::primitives::Header>
        + ChainSpecProvider<ChainSpec: Hardforks>
        + Unpin
        + Clone
        + 'static
{
    type FunctionOutput = StateMachineCheckHookFn<C>;

    fn check_block(&mut self, block_number: u64) {
        let f = move |testnet: &mut AngstromTestnet<C, DevnetConfig, WalletProvider>| {
            testnet.check_block_numbers(block_number)
        };
        self.add_check("check block", f);
    }

    fn check_token_price_gen_has_pools(
        &mut self,
        checked_pair_to_pool: HashMap<(Address, Address), PoolId>
    ) {
        let f = move |testnet: &mut AngstromTestnet<C, DevnetConfig, WalletProvider>| {
            let token_gen = testnet
                .random_peer()
                .strom_validation(|v| v.underlying.token_price_generator());

            let pairs_to_pools = token_gen.pairs_to_pools();
            let binding = token_gen.prev_prices();
            let prev_prices = binding.keys().sorted().collect::<Vec<_>>();

            let checked_pair_to_pool_ids =
                checked_pair_to_pool.values().sorted().collect::<Vec<_>>();

            Ok(prev_prices == checked_pair_to_pool_ids && checked_pair_to_pool == pairs_to_pools)
        };

        self.add_check("check token price gen has pools", f);
    }
}
