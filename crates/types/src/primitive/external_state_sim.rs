use alloy_primitives::{Address, Bytes};

use crate::rpc::SignedComposableLimitOrder;

pub type ExternalStateCall = (Address, Bytes);

#[derive(Debug)]
pub struct ExternalStateSim {
    pub tx:               SignedComposableLimitOrder,
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

impl TryInto<ExternalStateSim> for SignedComposableLimitOrder {
    type Error = anyhow::Error;

    fn try_into(self) -> Result<ExternalStateSim, Self::Error> {
        let addr: Address = self.recover_signer().unwrap();

        Ok(ExternalStateSim {
            tx: self.clone(),
            pre_hook: self.details.preHook.into(),
            amount_in_req: self.details.amountIn,
            amount_in_token: self.details.currencyIn,
            post_hook: self.details.postHook.into(),
            amount_out_lim: self.details.amountOutMin,
            amount_out_token: self.details.currencyOut,
            addr
        })
    }
}
