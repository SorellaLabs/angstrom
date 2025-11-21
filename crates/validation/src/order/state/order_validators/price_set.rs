use angstrom_types::{
    primitive::{OrderValidationError, Ray},
    sol_bindings::RawPoolOrder
};

use super::{OrderValidation, OrderValidationState};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct EnsurePriceSet;

impl OrderValidation for EnsurePriceSet {
    fn validate_order<O: RawPoolOrder>(
        &self,
        state: &mut OrderValidationState<O>
    ) -> Result<(), OrderValidationError> {
        if state.order().is_tob() {
            return Ok(());
        }

        if state.order().limit_price().is_zero() {
            Err(OrderValidationError::NoPriceSpecified)
        } else if !Ray::from(state.order().limit_price()).within_sqrt_price_bounds() {
            Err(OrderValidationError::PriceOutOfPoolBounds)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod test {

    use super::*;
    #[test]
    fn test_no_price_specified_error() {
        use angstrom_types::primitive::OrderValidationError;
        use revm_primitives::U256;

        use crate::order::{
            AllOrders,
            state::order_validators::{EnsurePriceSet, OrderValidationState, make_base_order}
        };

        let mut order = make_base_order();
        if let AllOrders::PartialStanding(ref mut o) = order {
            o.min_price = U256::from(0);
        }

        let validator = EnsurePriceSet;
        let mut state = OrderValidationState::new(&order);
        let result = validator.validate_order(&mut state);
        assert_eq!(result, Err(OrderValidationError::NoPriceSpecified));
    }
}
