use std::collections::HashMap;

use alloy_sol_types::sol;
use ethers_core::types::U256;
use reth_primitives::{bytes::BytesMut, H256};
use reth_rlp::Encodable;
use revm::primitives::{TransactTo, TxEnv, B160, U256 as RU256};
use serde::{Deserialize, Serialize};

use super::{
    CurrencySettlement, PoolFees, PoolSwap, RawLvrSettlement, RawUserSettlement, RlpDecodable,
    RlpEncodable, SimmedLvrSettlement, SimmedUserSettlement, ANGSTROM_CONTRACT_ADDR
};
use crate::on_chain::Order;

#[derive(Debug, Clone, Serialize, Deserialize, RlpDecodable, RlpEncodable, PartialEq, Eq, Hash)]
pub struct SimmedBundle {
    // Univ4 swaps
    pub raw:      Bundle,
    // metadata that shouldn't be encoded or taken into account for hash
    pub gas_used: U256
}

impl SimmedBundle {
    pub fn get_cumulative_lp_bribe(&self) -> u128 {
        self.raw.get_cumulative_lp_bribe()
    }

    pub fn hash(&self) -> H256 {
        let mut buf = BytesMut::new();
        self.raw.encode(&mut buf);

        H256(ethers_core::utils::keccak256(buf))
    }
}

sol! {
    type Currency is address;
    /// @notice Returns the key for identifying a pool
    struct PoolKey {
        /// @notice The lower currency of the pool, sorted numerically
        Currency currency0;
        /// @notice The higher currency of the pool, sorted numerically
        Currency currency1;
        /// @notice The pool swap fee, capped at 1_000_000. The upper 4 bits determine if the hook sets any fees.
        uint24 fee;
        /// @notice Ticks that involve positions must be a multiple of tick spacing
        int24 tickSpacing;
        /// @notice The hooks of the pool
        IHooks hooks;
    }


    /// @notice Complete Angstrom bundle.
    struct Bundle {
        /// @member All executed user orders.
        ExecutedOrder[] orders;
        /// @member Abi-encoded UniswapData.
        bytes uniswapData;
    }

    /// @notice Instruction to execute a swap on UniswapV4.
    struct PoolSwap {
        /// @member The pool to perform the swap on.
        PoolKey pool;
        /// @member The input currency.
        Currency currencyIn;
        /// @member The amount of input.
        uint256 amountIn;
    }

    /// @notice Signed order with actual execution amounts.
    struct ExecutedOrder {
        /// @member The original order from the user.
        Order order;
        /// @member The user's EIP-712 signature of the Order.
        bytes signature;
        /// @member The actual executed input amount.
        uint256 amountInActual;
        /// @member The actual executed output amount.
        uint256 amountOutActual;
    }

    /// @notice Instruction to settle an amount of currency.
    struct CurrencySettlement {
        /// @member The currency to settle.
        Currency currency;
        /// @member The amount to settle, positive indicates we must pay, negative
        ///         indicates we are paid.
        int256 amountNet;
    }

    /// @notice Instruction to donate revenue to a pool.
    struct PoolFees {
        /// @member The pool to pay fees to.
        PoolKey pool;
        /// @member The amount0 fee.
        uint256 fees0;
        /// @member The amount1 fee.
        uint256 fees1;
    }

    /// @notice Uniswap instructions to execute after lock is taken.
    struct UniswapData {
        /// @member The discrete swaps to perform, there should be at most one entry
        ///         per pool.
        PoolSwap[] swaps;
        /// @member The currency settlements to perform, there should be at most one
        ///         entry per currency.
        CurrencySettlement[] currencies;
        /// @member The fees to pay to each pool, there should be at most one entry
        ///         per pool.
        PoolFees[] pools;
    }

}

impl RawBundle {
    pub fn hash(&self) -> H256 {
        let mut buf = BytesMut::new();
        self.encode(&mut buf);

        H256(ethers_core::utils::keccak256(buf))
    }
}

impl From<SimmedBundle> for RawBundle {
    fn from(value: SimmedBundle) -> Self {
        value.raw
    }
}

impl Into<TxEnv> for RawBundle {
    fn into(self) -> TxEnv {
        let mut gas_limit = U256::from_dec_str("0").unwrap();
        let _ = self.lvr.iter().map(|v| gas_limit += v.raw.order.gas_cap);
        let _ = self.users.iter().map(|v| gas_limit += v.raw.order.gas_cap);

        let mut data = BytesMut::new();
        self.encode(&mut data);

        TxEnv {
            caller:           Default::default(),
            gas_limit:        gas_limit.as_u64(),
            gas_price:        reth_primitives::U256::ZERO,
            gas_priority_fee: None,
            transact_to:      TransactTo::Call(ANGSTROM_CONTRACT_ADDR.into()),
            value:            reth_primitives::U256::ZERO,
            data:             data.to_vec().into(),
            chain_id:         Some(1),
            nonce:            None,
            access_list:      Default::default()
        }
    }
}

impl RawBundle {
    pub fn get_cumulative_lp_bribe(&self) -> u128 {
        todo!()
    }
}

#[derive(Debug, Clone)]
pub struct CallerInfo {
    pub address:   B160,
    pub nonce:     u64,
    pub overrides: HashMap<B160, HashMap<RU256, RU256>>
}
