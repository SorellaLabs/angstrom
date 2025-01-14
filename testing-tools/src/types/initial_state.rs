use alloy::providers::PendingTransaction;
use alloy_primitives::{
    aliases::{I24, U24},
    Address, TxHash
};
use angstrom_types::{contract_bindings::angstrom::Angstrom::PoolKey, matching::SqrtPriceX96};

pub struct PendingDeployedPools {
    pending_txs: Vec<PendingTransaction>,
    pool_keys:   Vec<PoolKey>
}

impl Default for PendingDeployedPools {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingDeployedPools {
    pub fn new() -> Self {
        Self { pending_txs: Vec::new(), pool_keys: Vec::new() }
    }

    pub fn add_pending_tx(&mut self, tx: PendingTransaction) {
        self.pending_txs.push(tx)
    }

    pub fn add_pool_key(&mut self, pool_key: PoolKey) {
        self.pool_keys.push(pool_key)
    }

    pub fn pool_keys(&self) -> &[PoolKey] {
        &self.pool_keys
    }

    pub async fn finalize_pending_txs(&mut self) -> eyre::Result<(Vec<PoolKey>, Vec<TxHash>)> {
        let keys = std::mem::take(&mut self.pool_keys);

        let tx_hashes = futures::future::join_all(std::mem::take(&mut self.pending_txs))
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        Ok((keys, tx_hashes))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PartialConfigPoolKey {
    // currency0:         Address,
    // currency1:         Address,
    pub fee:               u64,
    pub tick_spacing:      i32,
    pub initial_liquidity: u128,
    pub sqrt_price:        SqrtPriceX96
}

impl PartialConfigPoolKey {
    pub fn new(
        // currency0: Address,
        // currency1: Address,
        fee: u64,
        tick_spacing: i32,
        initial_liquidity: u128,
        sqrt_price: SqrtPriceX96
    ) -> Self {
        Self { fee, tick_spacing, initial_liquidity, sqrt_price }
    }

    pub fn make_pool_key(
        &self,
        angstrom_address_hook: Address,
        cur0: Address,
        cur1: Address
    ) -> PoolKey {
        PoolKey {
            currency0:   cur0,
            currency1:   cur1,
            fee:         U24::from(self.fee),
            tickSpacing: I24::unchecked_from(self.tick_spacing),
            hooks:       angstrom_address_hook
        }
    }

    pub fn initial_liquidity(&self) -> u128 {
        self.initial_liquidity
    }

    pub fn sqrt_price(&self) -> SqrtPriceX96 {
        self.sqrt_price
    }
}
