use std::collections::HashMap;

use alloy::primitives::Address;

/// Tracks the state of our borrowing from Uniswap for a particular phase of
/// contract execution.
///
/// FLAW - Right now we always do the Uniswap swap first so this is not
/// manifested, however this currently does not delineate between loans we
/// voluntarily take and Uniswap swaps we MUST execute to properly push the
/// price.  Combining steps can reduce uniswap swaps based on liquidity which is
/// mathematically correct (i.e. we will have the numbers needed to fulfill
/// orders) but will not move the Uniswap price to where we want it to be.
#[derive(Default, Debug, Clone)]
pub struct BorrowStateTracker {
    pub take:            u128,
    pub contract_liquid: u128,
    pub settle:          u128,
    pub save:            u128
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

    /// Add to what we're taking for ourself in terms of "rewards" from
    /// donations
    pub fn reward(&mut self, q: u128) {
        self.take += q;
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

    pub fn add_gas_fee(&mut self, q: u128) {
        self.save += q;
    }

    /// Combines this borrow state with another borrow state that is expected to
    /// describe operations chronologically following this one
    pub fn and_then(&self, other: &Self) -> Self {
        // The amount that we will need to borrow is the amount we had to borrow for
        // this step plus the amount we need to borrow for the next step MINUS the
        // amount we had liquid at the end of this step (as that will be available to
        // the next step)
        let borrow_needed = self.take + (other.take.saturating_sub(self.contract_liquid));
        // The amount we'll have on-hand is equal to the amount we currently have
        // on-hand minus the amount the next stage would have needed to borrow plus the
        // amount the next stage will end with on hand
        let amount_onhand =
            (self.contract_liquid.saturating_sub(other.take)) + other.contract_liquid;
        // The amount we owe back to Uniswap is the amount we currently owe to uniswap
        // plus the amount the next step owes to uniswap MINUS the amount we had liquid
        // at the end of this step (as that will be available to the next step)
        let amount_owed = self.settle + (other.settle.saturating_sub(self.contract_liquid));
        // The amount we're saving for later just adds up
        let amount_save = self.save + other.save;

        Self {
            take:            borrow_needed,
            contract_liquid: amount_onhand,
            settle:          amount_owed,
            save:            amount_save
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

    pub fn add_gas_fee(&mut self, asset: Address, qty: u128) {
        self.get_state(asset).add_gas_fee(qty);
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

    /// Accept "tribute" that will go to the Angstrom contract overall
    pub fn tribute(&mut self, asset: Address, q: u128) {
        self.get_state(asset).reward(q);
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

    /// Take all residual contract liquidity and transfer it to `save` for our
    /// collection
    pub fn collect_extra(&self) -> Self {
        let new_map = self
            .map
            .iter()
            .map(|(addr, state)| {
                println!("--- {:?} ---\n{:#?}\n----NEW STATE------", addr, state);
                // Any liquidity we have over what it will take us to settle should be saved for
                // our contract.  This should usually just be scraps and rounding errors
                let extra_liquidity = state.contract_liquid.saturating_sub(state.settle);
                let new_state = BorrowStateTracker {
                    take:            state.take,
                    contract_liquid: 0,
                    save:            state.save + extra_liquidity,
                    settle:          state.settle
                };
                println!("{:#?}\n------------", new_state);
                (*addr, new_state)
            })
            .collect();
        Self { map: new_map }
    }
}
