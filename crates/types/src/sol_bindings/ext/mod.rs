//! extension functionality to sol types
use std::{cmp::Ordering, fmt};

use alloy::primitives::{Address, FixedBytes, TxHash, U256};
use alloy_primitives::PrimitiveSignature;
use serde::{Deserialize, Serialize};

use crate::{orders::OrderLocation, sol_bindings::Ray};

pub mod flips;
pub mod grouped_orders;

/// The capability of all default orders.
pub trait RawPoolOrder: fmt::Debug + Send + Sync + Clone + Unpin + 'static {
    fn max_gas_token_0(&self) -> u128;
    /// defines  
    /// Hash of the order
    fn order_hash(&self) -> TxHash;

    /// The order signer
    fn from(&self) -> Address;

    /// the amount specified by the user. if the order is a partial, this is the
    /// max
    // /value
    fn amount(&self) -> u128;

    /// the min qty of t0 specified by the user
    fn min_qty_t0(&self) -> Option<u128> {
        if self.amount() == 0 || self.limit_price() == U256::ZERO {
            return None;
        }

        Some(if !self.is_bid() {
            if self.exact_in() {
                self.min_amount()
            } else {
                Ray::from(self.limit_price()).inverse_quantity(self.min_amount(), true)
            }
        } else if self.exact_in() {
            Ray::from(self.limit_price())
                .mul_quantity(U256::from(self.min_amount()))
                .to::<u128>()
        } else {
            self.min_amount()
        })
    }

    /// ensure to override for variants that have tob.
    fn is_tob(&self) -> bool {
        false
    }

    /// if the order is partial. If the min amount == max amount but order is
    /// specified as partial, we don't give it priority ordering
    fn is_partial(&self) -> bool {
        self.min_amount() < self.amount()
    }

    /// the amount specified by the user. if the order is a partial, this is the
    /// min value. otherwise it is the same as amount
    fn min_amount(&self) -> u128;

    /// Limit Price
    fn limit_price(&self) -> U256;

    /// Order deadline
    fn deadline(&self) -> Option<U256>;
    /// order flash block
    fn flash_block(&self) -> Option<u64>;

    /// the way in which we avoid a respend attack
    fn respend_avoidance_strategy(&self) -> RespendAvoidanceMethod;

    /// when validating, the priority in which we sort orders to allocate
    /// the total balance and approval.
    fn validation_priority(&self) -> OrderValidationPriority {
        OrderValidationPriority {
            order_hash: self.order_hash(),
            is_tob:     self.is_tob(),
            is_partial: self.is_partial(),
            respend:    self.respend_avoidance_strategy()
        }
    }

    /// token in
    fn token_in(&self) -> Address;
    /// token out
    fn token_out(&self) -> Address;

    /// An order is a bid if it's putting in T1 to get out T0.  T1 is always the
    /// greater address, so if `token_in > token_out` then T1 is being put in to
    /// get T0 out and this order is a bid
    fn is_bid(&self) -> bool {
        self.token_in() > self.token_out()
    }

    fn is_valid_signature(&self) -> bool;

    fn order_location(&self) -> OrderLocation;

    /// whether to use angstrom balances or not
    fn use_internal(&self) -> bool;

    fn order_signature(&self) -> eyre::Result<PrimitiveSignature>;

    fn exact_in(&self) -> bool;

    fn has_hook(&self) -> bool;
}

pub trait GenerateFlippedOrder: Send + Sync + Clone + Unpin + 'static {
    fn flip(&self) -> Self
    where
        Self: Sized;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash, Copy)]
pub enum RespendAvoidanceMethod {
    Nonce(u64),
    Block(u64)
}

impl Default for RespendAvoidanceMethod {
    fn default() -> Self {
        Self::Nonce(0)
    }
}

impl RespendAvoidanceMethod {
    pub fn get_ord_for_pending_orders(&self) -> u64 {
        let Self::Nonce(n) = self else { return 0 };
        *n
    }
}

#[derive(Clone, Debug, Copy)]
pub struct OrderValidationPriority {
    pub order_hash: FixedBytes<32>,
    pub is_tob:     bool,
    pub is_partial: bool,
    pub respend:    RespendAvoidanceMethod
}

impl OrderValidationPriority {
    pub fn is_higher_priority(&self, other: &Self) -> Ordering {
        self.is_tob.cmp(&other.is_tob).then_with(|| {
            self.is_partial.cmp(&other.is_partial).then_with(|| {
                other
                    .respend
                    .get_ord_for_pending_orders()
                    .cmp(&self.respend.get_ord_for_pending_orders())
                    .then_with(|| other.order_hash.cmp(&self.order_hash))
            })
        })
    }
}

#[cfg(test)]
mod order_validation_priority_tests {
    use alloy::primitives::B256;

    use super::*;

    fn create_priority(is_tob: bool, is_partial: bool, nonce: u64) -> OrderValidationPriority {
        OrderValidationPriority {
            order_hash: B256::random(),
            is_tob,
            is_partial,
            respend: RespendAvoidanceMethod::Nonce(nonce)
        }
    }

    #[test]
    fn test_tob_has_highest_priority() {
        // TOB orders should have higher priority than non-TOB orders
        let tob_order = create_priority(true, false, 1);
        let non_tob_order = create_priority(false, false, 1);

        assert_eq!(tob_order.is_higher_priority(&non_tob_order), Ordering::Greater);
        assert_eq!(non_tob_order.is_higher_priority(&tob_order), Ordering::Less);
    }

    #[test]
    fn test_partial_has_higher_priority_than_non_partial() {
        // For same TOB status, non-partial orders should have higher priority
        let non_partial = create_priority(false, false, 1);
        let partial = create_priority(false, true, 1);

        assert_eq!(partial.is_higher_priority(&non_partial), Ordering::Greater);
        assert_eq!(non_partial.is_higher_priority(&partial), Ordering::Less);
    }

    #[test]
    fn test_lower_nonce_has_higher_priority() {
        // For same TOB and partial status, lower nonce should have higher priority
        let lower_nonce = create_priority(true, false, 1);
        let higher_nonce = create_priority(true, false, 2);

        assert_eq!(lower_nonce.is_higher_priority(&higher_nonce), Ordering::Greater);
        assert_eq!(higher_nonce.is_higher_priority(&lower_nonce), Ordering::Less);
    }

    #[test]
    fn test_order_hash_tiebreaker() {
        // Create two orders with identical properties except hash
        let order1 = OrderValidationPriority {
            order_hash: B256::with_last_byte(1),
            is_tob:     true,
            is_partial: false,
            respend:    RespendAvoidanceMethod::Nonce(1)
        };

        let order2 = OrderValidationPriority {
            order_hash: B256::with_last_byte(2),
            is_tob:     true,
            is_partial: false,
            respend:    RespendAvoidanceMethod::Nonce(1)
        };

        // The order with the smaller hash should have higher priority
        let expected_ordering = order2.order_hash.cmp(&order1.order_hash);
        assert_eq!(order1.is_higher_priority(&order2), expected_ordering);
    }

    #[test]
    fn test_priority_hierarchy() {
        let high = create_priority(true, false, 1);
        let medium = create_priority(true, false, 2);
        let low = create_priority(false, true, 1);
        let lowest = create_priority(false, false, 1);

        assert_eq!(high.is_higher_priority(&medium), Ordering::Greater);
        assert_eq!(medium.is_higher_priority(&low), Ordering::Greater);
        assert_eq!(low.is_higher_priority(&lowest), Ordering::Greater);

        // Test transitive property
        assert_eq!(high.is_higher_priority(&medium), Ordering::Greater);
        assert_eq!(high.is_higher_priority(&low), Ordering::Greater);
        assert_eq!(high.is_higher_priority(&lowest), Ordering::Greater);
    }

    #[test]
    fn test_block_respend_avoidance() {
        // Create orders with Block respend avoidance method
        let order1 = OrderValidationPriority {
            order_hash: B256::random(),
            is_tob:     true,
            is_partial: false,
            respend:    RespendAvoidanceMethod::Block(100)
        };

        let order2 = OrderValidationPriority {
            order_hash: B256::random(),
            is_tob:     true,
            is_partial: false,
            respend:    RespendAvoidanceMethod::Block(200)
        };

        // Block respend avoidance should return 0 for ordering
        // So the comparison should fall back to order_hash
        let expected_ordering = order2.order_hash.cmp(&order1.order_hash);
        assert_eq!(order1.is_higher_priority(&order2), expected_ordering);
    }

    #[test]
    fn test_mixed_respend_avoidance_methods() {
        // Create orders with different respend avoidance methods
        let nonce_order = OrderValidationPriority {
            order_hash: B256::random(),
            is_tob:     true,
            is_partial: false,
            respend:    RespendAvoidanceMethod::Nonce(1)
        };

        let block_order = OrderValidationPriority {
            order_hash: B256::random(),
            is_tob:     true,
            is_partial: false,
            respend:    RespendAvoidanceMethod::Block(100)
        };

        // Nonce(1) should return 1, Block should return 0
        // So nonce_order should have lower priority
        assert_eq!(nonce_order.is_higher_priority(&block_order), Ordering::Less);
        assert_eq!(block_order.is_higher_priority(&nonce_order), Ordering::Greater);
    }
}
