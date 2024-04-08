use alloy_primitives::{Address, Bytes};

use crate::rpc::SignedLimitOrder;

pub type ExternalStateCall = (Address, Bytes);

#[derive(Debug)]
pub struct ExternalStateSim {
    pub tx:               SignedLimitOrder,
    // the address of the user.
    pub addr:             Address,
    // gas in
    pub pre_hook:         Bytes,
    pub amount_in_req:    u128,
    pub amount_in_token:  Address,
    // gas out
    pub post_hook:        Bytes,
    pub amount_out_lim:   u128,
    pub amount_out_token: Address
}

impl ExternalStateSim {
    pub fn pre_hook(&self) -> ExternalStateCall {
        let addr = Address::from_slice(&self.pre_hook[0..20]);

        (addr, Bytes::copy_from_slice(&self.pre_hook[21..]))
    }

    pub fn post_hook(&self) -> ExternalStateCall {
        let addr = Address::from_slice(&self.post_hook[0..20]);

        (addr, Bytes::copy_from_slice(&self.post_hook[21..]))
    }
}

impl TryInto<ExternalStateSim> for SignedLimitOrder {
    type Error = anyhow::Error;

    fn try_into(self) -> Result<ExternalStateSim, Self::Error> {
        let addr: Address = self.recover_signer().unwrap();

        Ok(ExternalStateSim {
            tx: self.clone(),
            pre_hook: self.order.preHook,
            amount_in_req: self.order.amountIn,
            amount_in_token: self.order.currencyIn,
            post_hook: self.order.postHook,
            amount_out_lim: self.order.amountOutMin,
            amount_out_token: self.order.currencyOut,
            addr
        })
    }
}
