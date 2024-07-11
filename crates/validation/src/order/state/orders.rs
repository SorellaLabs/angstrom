use std::{collections::HashMap, fmt::Debug};

use alloy_primitives::{Address, U256};
use angstrom_types::{
    orders::{OrderLocation, OrderPriorityData, StateValidationError, ValidationError},
    sol_bindings::{
        grouped_orders::{GroupedVanillaOrder, OrderWithStorageData},
        sol::TopOfBlockOrder
    }
};

use super::upkeepers::UserAccountDetails;

type Amount = U256;
type OrderNonce = U256;
type UserAddress = Address;

/// the sum of all pending orders for a given user. This is done
/// so that validation of specific orders is not dependant on all other orders.
#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct PendingState {
    token_balances:  HashMap<Address, Amount>,
    token_approvals: HashMap<Address, Amount>
}

pub struct UserOrders(HashMap<UserAddress, (PendingState, Vec<OrderNonce>)>);

impl Default for UserOrders {
    fn default() -> Self {
        Self::new()
    }
}

impl UserOrders {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn new_searcher_order(
        &mut self,
        order: TopOfBlockOrder,
        deltas: UserAccountDetails,
        block_number: u64
    ) -> OrderWithStorageData<TopOfBlockOrder> {
        todo!()
    }

    pub fn new_limit_order(
        &mut self,
        order: GroupedVanillaOrder,
        deltas: UserAccountDetails,
        block_number: u64
    ) -> OrderWithStorageData<GroupedVanillaOrder> {
        todo!()
        // self.basic_order_validation(order, deltas, true, block_number,
        // |order| OrderPriorityData {     price:  order.limit_price(),
        //     volume:
        // order.limit_price().saturating_mul(order.amount_out_min()),
        //     gas:    order.gas()
        // })
    }

    /// called when a user has a state change on their address. When this
    /// happens we re-evaluate all of there pending orders so we do a
    /// hard-reset here.
    pub fn fkresh_state(&mut self, state: HashMap<Address, PendingState>) {
        state.into_iter().for_each(|(k, v)| {
            self.0.insert(k, (v, vec![]));
        });
    }

    // fn basic_order_validation<
    //     O: PoolOrder<ValidationData = Data>,
    //     Data: Send + Debug + Sync + Clone + Unpin + 'static,
    //     F: FnOnce(&O) -> Data
    // >(
    //     &mut self,
    //     order: O,
    //     deltas: UserAccountDetails,
    //     limit: bool,
    //     block_number: u64,
    //     build_priority: F
    // ) -> OrderValidationOutcome<O> {
    //     tracing::debug!(?deltas);
    //     // always invalid
    //     if !deltas.is_valid_nonce
    //         || !deltas.is_valid_pool
    //         || self.has_nonce_overlap(&order.from(), &order.nonce())
    //     {
    //         let hash = order.hash();
    //         let nonce = order.nonce();
    //         return OrderValidationOutcome::Invalid(
    //             order,
    //
    // ValidationError::StateValidationError(StateValidationError::InvalidNonce(
    //                 hash, nonce
    //             ))
    //         )
    //     }
    //
    //     let user = order.from();
    //     let (pending_state, ids) = self.0.entry(user).or_default();
    //     ids.push(order.nonce());
    //
    //     // insert approvals if empty
    //     pending_state
    //         .token_approvals
    //         .entry(deltas.token_approvals.0)
    //         .or_insert(deltas.token_approvals.1);
    //
    //     // insert balance if empty
    //     pending_state
    //         .token_balances
    //         .entry(deltas.token_bals.0)
    //         .or_insert(deltas.token_bals.1);
    //
    //     // track which pool this should go into
    //     let mut has_balances = true;
    //
    //     // subtract token in from approval
    //     if let Some(token) =
    // pending_state.token_approvals.get_mut(&order.token_in()) {         if
    // (*token)             .checked_sub(U256::from(order.amount_in()))
    //             .is_none()
    //         {
    //             has_balances = false;
    //         } else {
    //             *token -= U256::from(order.amount_in());
    //         }
    //     } else {
    //         has_balances = false;
    //     }
    //
    //     // if approvals passed check balances
    //     if has_balances {
    //         if let Some(token) =
    // pending_state.token_balances.get_mut(&order.token_in()) {             if
    // (*token)                 .checked_sub(U256::from(order.amount_in()))
    //                 .is_none()
    //             {
    //                 // add balance back to approval
    //                 // NOTE: default will never be called here
    //                 *pending_state
    //                     .token_approvals
    //                     .entry(order.token_in())
    //                     .or_default() += U256::from(order.amount_in());
    //
    //                 has_balances = false;
    //             } else {
    //                 *token -= U256::from(order.amount_in());
    //             }
    //         } else {
    //             // NOTE: default will never be called here
    //             *pending_state
    //                 .token_approvals
    //                 .entry(order.token_in())
    //                 .or_default() += U256::from(order.amount_in());
    //
    //             has_balances = false;
    //         }
    //     }
    //     // NOTE: because we can't guarentee the order of execution with
    //     // these orders, we cannot add the amount out balance to
    //     // token balances to allow multi hop with different
    //     // intents within a single transaction
    //
    //     let data = build_priority(&order);
    //
    //     let res = ValidatedOrder {
    //         order,
    //         data,
    //         is_bid: deltas.is_bid,
    //         pool_id: deltas.pool_id,
    //         location: if limit {
    //             if has_balances {
    //                 OrderLocation::Limit
    //             } else {
    //                 OrderLocation::Limit
    //             }
    //         } else {
    //             OrderLocation::Searcher
    //         }
    //     };
    //
    //     OrderValidationOutcome::Valid { order: res, propagate: true, block_number
    // } }

    /// Helper function for checking for duplicates when adding orders
    fn has_nonce_overlap(&self, address: &Address, id: &U256) -> bool {
        self.0
            .get(address)
            .map(|inner| inner.1.iter().any(|other_id| other_id == id))
            .unwrap_or(false)
    }
}
