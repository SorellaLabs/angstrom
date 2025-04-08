use angstrom_types::{
    primitive::OrderValidationError,
    sol_bindings::{RawPoolOrder, Ray}
};

use super::{OrderValidation, OrderValidationState};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct EnsurePriceSet;

impl OrderValidation for EnsurePriceSet {
    fn validate_order<O: RawPoolOrder>(
        &self,
        state: &mut OrderValidationState<O>
    ) -> Result<(), OrderValidationError> {
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
        use angstrom_types::{
            primitive::OrderValidationError, sol_bindings::grouped_orders::StandingVariants
        };
        use revm_primitives::U256;

        use crate::order::{
            GroupedVanillaOrder,
            state::order_validators::{EnsurePriceSet, OrderValidationState, make_base_order}
        };

        let mut order = make_base_order();
        if let GroupedVanillaOrder::Standing(StandingVariants::Partial(ref mut o)) = order {
            o.min_price = U256::from(0);
        }

        let validator = EnsurePriceSet;
        let mut state = OrderValidationState::new(&order);
        let result = validator.validate_order(&mut state);
        assert_eq!(result, Err(OrderValidationError::NoPriceSpecified));
    }
}
