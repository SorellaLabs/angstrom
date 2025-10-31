use std::collections::HashMap;

use alloy_primitives::{Address, aliases::U24};
use angstrom_types::{contract_payloads::angstrom::AngstromPoolPartialKey, primitive::PoolId};
use itertools::Itertools;
use reth_chainspec::Hardforks;
use reth_provider::{BlockReader, ChainSpecProvider, HeaderProvider, ReceiptProvider};

use crate::{
    controllers::enviroments::{AngstromTestnet, DevnetStateMachine},
    providers::{WalletProvider, utils::async_to_sync},
    types::{StateMachineCheckHookFn, config::DevnetConfig}
};

pub trait WithCheck<C>
where
    C: BlockReader<Block = reth_primitives::Block>
        + ReceiptProvider<Receipt = reth_primitives::Receipt>
        + HeaderProvider<Header = reth_primitives::Header>
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

    fn check_pool_fees(
        &mut self,
        store_key: AngstromPoolPartialKey,
        bundle_fee: U24,
        unlock_fee: U24,
        protocol_unlock_fee: U24
    );
}

impl<C> WithCheck<C> for DevnetStateMachine<'_, C>
where
    C: BlockReader<Block = reth_primitives::Block>
        + ReceiptProvider<Receipt = reth_primitives::Receipt>
        + HeaderProvider<Header = reth_primitives::Header>
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

    fn check_pool_fees(
        &mut self,
        store_key: AngstromPoolPartialKey,
        bundle_fee: U24,
        unlock_fee: U24,
        protocol_unlock_fee: U24
    ) {
        let f = move |testnet: &mut AngstromTestnet<C, DevnetConfig, WalletProvider>| {
            let (c_bundle_fee, c_unlock_fee, c_protocol_unlocked_fee) =
                async_to_sync(testnet.get_pool_fees(store_key))?;

            Ok(c_bundle_fee == bundle_fee
                && c_unlock_fee == unlock_fee
                && c_protocol_unlocked_fee == protocol_unlock_fee)
        };

        self.add_check("check pool fees are expected", f);
    }
}
