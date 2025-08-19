mod fillstate;
mod origin;
use alloy::primitives::{Address, B256, Signature};
pub mod orderpool;

use alloy_primitives::{Bytes, I256};
pub use fillstate::*;
pub use orderpool::*;
pub use origin::*;
use pade::{PadeDecode, PadeEncode};
use serde::{Deserialize, Serialize};

pub type BookID = u128;
pub type OrderID = u128;
pub type OrderVolume = u128;
pub type OrderPrice = MatchingPrice;

use crate::{
    matching::{MatchingPrice, Ray, uniswap::Direction},
    primitive::{AngstromMetaSigner, AngstromSigner, PoolId},
    sol_bindings::{
        grouped_orders::{AllOrders, OrderWithStorageData},
        rpc_orders::TopOfBlockOrder
    }
};

#[derive(Debug)]
pub struct OrderSet<Limit, Searcher> {
    pub limit:    Vec<OrderWithStorageData<Limit>>,
    pub searcher: Vec<OrderWithStorageData<Searcher>>
}

impl<Limit, Searcher> OrderSet<Limit, Searcher>
where
    Limit: Clone,
    Searcher: Clone,
    AllOrders: From<Searcher> + From<Limit>
{
    pub fn total_orders(&self) -> usize {
        self.limit.len() + self.searcher.len()
    }

    pub fn into_all_orders(&self) -> Vec<AllOrders> {
        self.limit
            .clone()
            .into_iter()
            .map(|o| o.order.into())
            .chain(self.searcher.clone().into_iter().map(|o| o.order.into()))
            .collect()
    }

    /// Convert thee order set to the limit and searcher orders, without any
    /// filtering.
    pub fn into_all_book_and_searcher(
        self
    ) -> (Vec<OrderWithStorageData<Limit>>, Vec<OrderWithStorageData<Searcher>>) {
        (self.limit, self.searcher)
    }

    pub fn into_book_and_searcher(
        self,
        valid_limit: Vec<B256>,
        valid_searcher: Vec<B256>
    ) -> (Vec<OrderWithStorageData<Limit>>, Vec<OrderWithStorageData<Searcher>>) {
        (
            self.limit
                .into_iter()
                .filter(|order| valid_limit.contains(&order.order_id.hash))
                .collect(),
            self.searcher
                .into_iter()
                .filter(|order| valid_searcher.contains(&order.order_id.hash))
                .collect()
        )
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetAmmOrder {
    /// A NetAmmOrder that is Buying will be purchasing T0 from the AMM
    Buy(u128, u128),
    /// A NetAmmOrder that is Selling will be selling T0 to the AMM
    Sell(u128, u128)
}

impl Default for NetAmmOrder {
    fn default() -> Self {
        Self::Buy(0, 0)
    }
}

impl NetAmmOrder {
    pub fn new(direction: Direction) -> Self {
        match direction {
            Direction::BuyingT0 => Self::Buy(0, 0),
            Direction::SellingT0 => Self::Sell(0, 0)
        }
    }

    pub fn right_direction(&self, direction: Direction) -> bool {
        match direction {
            Direction::BuyingT0 => matches!(self, Self::Sell(_, _)),
            Direction::SellingT0 => matches!(self, Self::Buy(_, _))
        }
    }

    pub fn add_quantity(&mut self, quantity: u128, cost: u128) {
        let (my_quantity, my_cost) = match self {
            Self::Buy(q, c) => (q, c),
            Self::Sell(q, c) => (q, c)
        };
        *my_cost += cost;
        *my_quantity += quantity;
    }

    pub fn remove_quantity(&mut self, quantity: u128, cost: u128) {
        let (my_quantity, my_cost) = match self {
            Self::Buy(q, c) => (q, c),
            Self::Sell(q, c) => (q, c)
        };
        *my_cost -= cost;
        *my_quantity -= quantity;
    }

    pub fn get_directions(&self) -> (u128, u128) {
        match self {
            Self::Buy(amount_out, amount_in) => (*amount_in, *amount_out),
            Self::Sell(amount_in, amount_out) => (*amount_in, *amount_out)
        }
    }

    /// Gets the net AMM order as a signed quantity of T0.  The quantity is
    /// positive if we are purchasing T0 from the AMM and negative if we are
    /// selling T0 into the AMM
    pub fn get_t0_signed(&self) -> I256 {
        tracing::trace!(net_amm_order = ?self, "Processing net AMM order into t0signed");
        match self {
            Self::Buy(t0, _) => I256::unchecked_from(*t0),
            Self::Sell(t0, _) => I256::unchecked_from(*t0).saturating_neg()
        }
    }

    pub fn amount_in(&self) -> u128 {
        self.get_directions().0
    }

    pub fn amount_out(&self) -> u128 {
        self.get_directions().1
    }

    pub fn to_order_tuple(&self, t0_idx: u16, t1_idx: u16) -> (u16, u16, u128, u128) {
        match self {
            NetAmmOrder::Buy(q, c) => (t1_idx, t0_idx, *c, *q),
            NetAmmOrder::Sell(q, c) => (t0_idx, t1_idx, *q, *c)
        }
    }

    pub fn is_bid(&self) -> bool {
        matches!(self, Self::Buy(_, _))
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderOutcome {
    pub id:      OrderId,
    pub outcome: OrderFillState
}

impl OrderOutcome {
    pub fn is_filled(&self) -> bool {
        self.outcome.is_filled()
    }

    pub fn fill_amount(&self, max: u128) -> u128 {
        match self.outcome {
            OrderFillState::CompleteFill => max,
            OrderFillState::PartialFill(p) => std::cmp::min(max, p),
            _ => 0
        }
    }
}

#[derive(Debug, Clone, Default, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolSolution {
    /// Id of this pool
    pub id:           PoolId,
    /// Uniform clearing price in Ray format
    pub ucp:          Ray,
    /// Winning searcher order to be executed
    pub searcher:     Option<OrderWithStorageData<TopOfBlockOrder>>,
    /// Quantity to be bought or sold from the amm
    pub amm_quantity: Option<NetAmmOrder>,
    /// IDs of limit orders to be executed - it might be easier to just use
    /// hashes here
    pub limit:        Vec<OrderOutcome>,
    /// Any additional reward quantity to be taken out of excess T0 after all
    /// other operations
    pub reward_t0:    u128,
    // fee on user orders.
    pub fee:          u32
}

impl PartialOrd for PoolSolution {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PoolSolution {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id.cmp(&other.id)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CancelOrderRequest {
    /// the signature encoded v,r,s in bytes.
    pub signature:    Bytes,
    // if there's no salt to make this a unique signing hash. One can just
    // copy the signature of the order and id and it will verify
    pub user_address: Address,
    pub order_id:     B256
}

impl CancelOrderRequest {
    pub fn new<S: AngstromMetaSigner>(
        user_address: Address,
        order_id: B256,
        signer: &AngstromSigner<S>
    ) -> Self {
        let payload = format!("canceling order: {order_id:?} for user: {user_address:?}");
        let signature = signer.sign_message_sync(payload.as_bytes()).unwrap();
        let encoded: Bytes = signature.pade_encode().into();

        Self { signature: encoded, user_address, order_id }
    }

    fn signing_payload(&self) -> String {
        format!("canceling order: {:?} for user: {:?}", self.order_id, self.user_address)
    }

    pub fn is_valid(&self) -> bool {
        let msg = self.signing_payload();
        let signature = self.signature.to_vec();
        let slice = &mut signature.as_slice();
        let Ok(signature) = Signature::pade_decode(slice, None) else {
            return false;
        };

        let Ok(sender) = signature.recover_address_from_msg(msg) else { return false };

        sender == self.user_address
    }
}

#[cfg(test)]
mod test {
    use alloy::hex;

    use super::*;
    use crate::primitive::AngstromSigner;

    #[test]
    fn ensure_cancel_order_works() {
        let wallet = AngstromSigner::random();
        let user = wallet.address();
        let order_id = B256::random();

        let cancel_order = CancelOrderRequest::new(user, order_id, &wallet);

        assert!(cancel_order.is_valid());
    }

    #[test]
    fn test_for_frontend() {
        let order_id = alloy::primitives::b256!(
            "0xbfef52d152545f5576f577dfe6f42984658c60ee39bdbbfa7d075d96d40a26c7"
        );
        let address = alloy::primitives::address!("0xcc0bff7564a892045667a68673220116ece65d6f");
        let bytes:Bytes = hex!("0x1c92d4a408d7ba4e41d0d454826f9e981eafe07308af502f62981683ea8bf052a24ca3a7846222225dc2526616440129f1023e4b1ae27f8089c8cab1bd60297ea7").into();

        let cancel = CancelOrderRequest { signature: bytes, user_address: address, order_id };

        assert!(cancel.is_valid());
    }

    #[test]
    fn test_new_funcion() {
        let wallet = AngstromSigner::random();
        let order_id = alloy::primitives::b256!(
            "0xbfef52d152545f5576f577dfe6f42984658c60ee39bdbbfa7d075d96d40a26c7"
        );
        let request = CancelOrderRequest::new(wallet.address(), order_id, &wallet);

        assert!(request.is_valid())
    }
}
