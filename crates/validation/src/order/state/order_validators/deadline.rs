use std::time::Duration;

use alloy::primitives::U256;
use angstrom_types::{
    primitive::{ETH_BLOCK_TIME, OrderValidationError},
    sol_bindings::RawPoolOrder
};

use super::{OrderValidation, OrderValidationState, clock::ValidationClock};

/// A deadline at or before this is expired: it cannot outlive the next block.
/// The pool prunes by the same horizon, so admission and pruning agree.
pub fn expiry_horizon_at(now: Duration) -> U256 {
    U256::from((now + ETH_BLOCK_TIME).as_secs())
}

/// The horizon on the system clock. Replay passes its own clock's `now()` to
/// [`expiry_horizon_at`] instead.
pub fn expiry_horizon() -> U256 {
    expiry_horizon_at(ValidationClock::System.now())
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct EnsureNotExpired;

impl OrderValidation for EnsureNotExpired {
    fn validate_order<O: RawPoolOrder>(
        &self,
        state: &mut OrderValidationState<O>
    ) -> Result<(), OrderValidationError> {
        match state.order().deadline() {
            Some(deadline) if deadline <= state.expiry_horizon() => {
                Err(OrderValidationError::Expired)
            }
            _ => Ok(())
        }
    }
}

#[cfg(test)]
mod test {
    use angstrom_types::sol_bindings::grouped_orders::AllOrders;
    use testing_tools::type_generator::orders::UserOrderBuilder;

    use super::*;
    use crate::order::state::order_validators::{ORDER_VALIDATORS, make_base_order};

    fn with_deadline(deadline: U256) -> AllOrders {
        let mut order = make_base_order();
        if let AllOrders::PartialStanding(ref mut o) = order {
            o.deadline = deadline.to();
        }
        order
    }

    fn validate(order: &AllOrders) -> Result<(), OrderValidationError> {
        EnsureNotExpired.validate_order(&mut OrderValidationState::new(order))
    }

    #[test]
    fn order_validation_rejects_an_expired_order() {
        let order = with_deadline(expiry_horizon());
        let mut state = OrderValidationState::new(&order);

        let result = ORDER_VALIDATORS
            .iter()
            .try_for_each(|validator| validator.validate_order(&mut state));

        assert_eq!(result, Err(OrderValidationError::Expired));
    }

    #[test]
    fn a_deadline_beyond_the_pruning_horizon_is_accepted() {
        let order = with_deadline(expiry_horizon() + U256::from(60));

        assert_eq!(validate(&order), Ok(()));
    }

    #[test]
    fn an_order_without_a_deadline_is_accepted() {
        let order = UserOrderBuilder::new()
            .kill_or_fill()
            .partial()
            .amount(1000)
            .block(100)
            .build();
        assert_eq!(order.deadline(), None);

        assert_eq!(validate(&order), Ok(()));
    }
}
