use std::collections::HashMap;

use alloy::primitives::Address;

#[derive(Default, Debug, Clone)]
pub struct BorrowStateTracker {
    pub take:            u128,
    pub contract_liquid: u128,
    pub settle:          u128
}

impl BorrowStateTracker {
    pub fn new() -> Self {
        Self { ..Default::default() }
    }

    /// Attempt to use tokens from contract liquidity.  If we don't have enough
    /// tokens there, add the difference to our Uniswap take/settle
    pub fn allocate(&mut self, q: u128) {
        if q > self.contract_liquid {
            let needed_borrow = q - self.contract_liquid;
            self.loan(needed_borrow);
        }
        self.contract_liquid = self.contract_liquid.saturating_sub(q);
    }

    /// Add to what we owe to Uniswap, as a result of a pool swap
    pub fn owe(&mut self, q: u128) {
        self.settle += q;
    }

    /// Add to what we take from Uniswap into our contract liquidity, as a
    /// result of a pool swap
    pub fn take(&mut self, q: u128) {
        self.take += q;
        self.contract_liquid += q;
    }

    /// Gain external tokens into our contract liquidity
    pub fn recieve(&mut self, q: u128) {
        self.contract_liquid += q;
    }

    /// Take a loan from Uniswap (adds to `take` and `settle`)
    pub fn loan(&mut self, q: u128) {
        self.take += q;
        self.settle += q;
    }

    pub fn and_then(&self, other: &Self) -> Self {
        let borrow_needed = self.take + (other.take.saturating_sub(self.contract_liquid));
        let amount_onhand =
            (self.contract_liquid.saturating_sub(other.take)) + other.contract_liquid;
        let amount_owed = self.settle + (other.settle.saturating_sub(self.contract_liquid));
        Self {
            take:            borrow_needed,
            contract_liquid: amount_onhand,
            settle:          amount_owed
        }
    }
}

#[derive(Debug, Default)]
pub struct StageTracker {
    map: HashMap<Address, BorrowStateTracker>
}

impl StageTracker {
    pub fn new() -> Self {
        Self { ..Default::default() }
    }

    pub fn get_asset(&self, asset: &Address) -> Option<&BorrowStateTracker> {
        self.map.get(asset)
    }

    #[inline]
    fn get_state(&mut self, addr: Address) -> &mut BorrowStateTracker {
        self.map.entry(addr).or_default()
    }

    /// Execute a swap that will take place on Uniswap
    pub fn uniswap_swap(
        &mut self,
        asset_in: Address,
        asset_out: Address,
        quantity_in: u128,
        quantity_out: u128
    ) {
        self.get_state(asset_in).owe(quantity_in);
        self.get_state(asset_out).take(quantity_out);
    }

    /// Execute a swap that comes from an external user - this is a ToB swap or
    /// a User Swap
    pub fn external_swap(
        &mut self,
        asset_in: Address,
        asset_out: Address,
        quantity_in: u128,
        quantity_out: u128
    ) {
        self.get_state(asset_in).recieve(quantity_in);
        self.get_state(asset_out).allocate(quantity_out);
    }

    pub fn allocate(&mut self, asset: Address, q: u128) {
        self.get_state(asset).allocate(q);
    }

    pub fn and_then(&self, other: &Self) -> Self {
        let mut new_map = self.map.clone();
        other.map.iter().for_each(|(addr, state)| {
            new_map
                .entry(*addr)
                .and_modify(|e| *e = e.and_then(state))
                .or_insert_with(|| state.clone());
        });
        Self { map: new_map }
    }
}
