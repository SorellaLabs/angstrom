use std::{fmt::Debug, sync::Arc};

use alloy::{consensus::Transaction, primitives::Address, providers::Provider, sol_types::SolCall};
use futures::{Stream, StreamExt};
use pade::PadeDecode;
use reth_provider::CanonStateNotificationStream;
use serde::{Deserialize, Serialize};

use crate::{
    contract_bindings::angstrom::Angstrom::executeCall,
    contract_payloads::angstrom::AngstromBundle, primitive::ChainExt, sol_bindings::Ray
};

/// represents the price settled on angstrom between two tokens
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct PairsWithPrice {
    pub token0:         Address,
    pub token1:         Address,
    pub price_1_over_0: Ray
}

impl PairsWithPrice {
    /// Decodes the AngstromPayload bundle and allows us to checkout
    /// the prices that the pools settled at. We then can use this for things
    /// such as our eth -> erc-20 gas price calculator
    pub fn from_angstrom_bundle(bundle: &AngstromBundle) -> Vec<Self> {
        bundle
            .pairs
            .iter()
            .map(|pair| Self {
                token0:         bundle.assets[pair.index0 as usize].addr,
                token1:         bundle.assets[pair.index1 as usize].addr,
                price_1_over_0: Ray::from(pair.price_1over0)
            })
            .collect::<Vec<_>>()
    }

    pub fn into_price_update_stream<P: Provider + 'static>(
        angstrom_address: Address,
        stream: CanonStateNotificationStream,
        provider: Arc<P>
    ) -> impl Stream<Item = (u128, Vec<Self>)> + Send {
        stream.then(move |notification| {
            let provider = provider.clone();
            async move {
                let new_cannon_chain = match notification {
                    reth_provider::CanonStateNotification::Reorg { new, .. } => new,
                    reth_provider::CanonStateNotification::Commit { new } => new
                };
                let gas_wei = provider.get_gas_price().await.unwrap_or_default();
                (
                    gas_wei,
                    new_cannon_chain
                        .successful_tip_transactions()
                        .filter(move |&tx| tx.to() == Some(angstrom_address))
                        .cloned()
                        .filter_map(|transaction| {
                            let input: &[u8] = transaction.input();
                            let b = executeCall::abi_decode(input).unwrap().encoded;
                            let mut bytes = b.as_ref();

                            AngstromBundle::pade_decode(&mut bytes, None).ok()
                        })
                        .flat_map(|bundle| Self::from_angstrom_bundle(&bundle))
                        .collect::<Vec<_>>()
                )
            }
        })
    }

    pub fn replace_price_if_empty(&mut self, pool_price_local: impl FnOnce() -> Ray) {
        if self.price_1_over_0.is_zero() {
            self.price_1_over_0 = pool_price_local();
        }
    }
}
