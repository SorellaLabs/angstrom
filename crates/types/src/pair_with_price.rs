use alloy::{consensus::Transaction, primitives::Address};
use futures::{Stream, StreamExt};
use pade::PadeDecode;
use reth_provider::CanonStateNotificationStream;

use crate::{contract_payloads::angstrom::AngstromBundle, sol_bindings::Ray};

/// represents the price settled on angstrom between two tokens
#[derive(Debug, Clone, Copy)]
pub struct PairsWithPrice {
    pub token0:         Address,
    pub token1:         Address,
    pub price_1_over_0: Ray,
    pub block_num:      u64
}

impl PairsWithPrice {
    /// Decodes the AngstromPayload bundle and allows us to checkout
    /// the prices that the pools settled at. We then can use this for things
    /// such as our eth -> erc-20 gas price calculator
    pub fn from_angstrom_bundle(block_num: u64, bundle: &AngstromBundle) -> Vec<Self> {
        bundle
            .pairs
            .iter()
            .map(|pair| Self {
                token0: bundle.assets[pair.index0 as usize].addr,
                token1: bundle.assets[pair.index1 as usize].addr,
                price_1_over_0: Ray::from(pair.price_1over0),
                block_num
            })
            .collect::<Vec<_>>()
    }

    pub fn into_price_update_stream(
        angstrom_address: Address,
        stream: CanonStateNotificationStream
    ) -> impl Stream<Item = Vec<Self>> + Send + Sync {
        stream.map(move |notification| {
            let new_cannon_chain = match notification {
                reth_provider::CanonStateNotification::Reorg { new, .. } => new,
                reth_provider::CanonStateNotification::Commit { new } => new
            };
            let block_num = new_cannon_chain.tip().number;
            new_cannon_chain
                .tip()
                .transactions()
                .iter()
                .filter(|tx| tx.transaction.to() == Some(angstrom_address))
                .filter_map(|transaction| {
                    let mut input: &[u8] = transaction.input();
                    AngstromBundle::pade_decode(&mut input, None).ok()
                })
                .take(1)
                .flat_map(|bundle| Self::from_angstrom_bundle(block_num, &bundle))
                .collect::<Vec<_>>()
        })
    }
}
