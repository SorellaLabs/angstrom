use amount_set::EnsureAmountSet;
use angstrom_types::{primitive::OrderValidationError, sol_bindings::RawPoolOrder};
use gas_set::EnsureGasSet;
use max_gas_lt_min::EnsureMaxGasLessThanMinAmount;
use price_set::EnsurePriceSet;

pub mod amount_set;
pub mod gas_set;
pub mod max_gas_lt_min;
pub mod price_set;

pub const ORDER_VALIDATORS: [OrderValidator; 4] = [
    OrderValidator::EnsureAmountSet(EnsureAmountSet),
    OrderValidator::EnsureGasSet(EnsureGasSet),
    OrderValidator::EnsurePriceSet(EnsurePriceSet),
    OrderValidator::EnsureMaxGasLessThanMinAmount(EnsureMaxGasLessThanMinAmount)
];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OrderValidationState<'a, O: RawPoolOrder> {
    order:   &'a O,
    min_qty: Option<u128>
}

impl<'a, O: RawPoolOrder> OrderValidationState<'a, O> {
    pub const fn new(order: &'a O) -> Self {
        Self { order, min_qty: None }
    }

    pub const fn order(&self) -> &'a O {
        self.order
    }

    pub fn min_qty_in_t0(&mut self) -> u128 {
        if let Some(min_qty) = self.min_qty {
            min_qty
        } else {
            let min_qty = self
                .order
                .min_qty_t0()
                .expect("amount and price checks should of been done before this was called");
            self.min_qty = Some(min_qty);

            min_qty
        }
    }
}

pub trait OrderValidation {
    fn validate_order<O: RawPoolOrder>(
        &self,
        state: &mut OrderValidationState<O>
    ) -> Result<(), OrderValidationError>;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OrderValidator {
    EnsureAmountSet(EnsureAmountSet),
    EnsureMaxGasLessThanMinAmount(EnsureMaxGasLessThanMinAmount),
    EnsurePriceSet(EnsurePriceSet),
    EnsureGasSet(EnsureGasSet)
}

impl OrderValidation for OrderValidator {
    fn validate_order<O: RawPoolOrder>(
        &self,
        state: &mut OrderValidationState<O>
    ) -> Result<(), OrderValidationError> {
        match self {
            OrderValidator::EnsureAmountSet(validator) => validator.validate_order(state),
            OrderValidator::EnsureMaxGasLessThanMinAmount(validator) => {
                validator.validate_order(state)
            }
            OrderValidator::EnsureGasSet(validator) => validator.validate_order(state),
            OrderValidator::EnsurePriceSet(validator) => validator.validate_order(state)
        }
    }
}

#[cfg(test)]
pub fn make_base_order() -> angstrom_types::sol_bindings::grouped_orders::GroupedVanillaOrder {
    use alloy::primitives::U256;
    use angstrom_types::sol_bindings::{
        Ray,
        grouped_orders::{GroupedVanillaOrder, StandingVariants}
    };
    use testing_tools::type_generator::orders::UserOrderBuilder;

    let mut order = match UserOrderBuilder::new()
        .standing()
        .partial()
        .amount(1000)
        .bid_min_price(Ray(U256::from(1)))
        .block(100)
        .deadline(U256::from(999_999))
        .nonce(0)
        .recipient(Default::default())
        .build()
    {
        GroupedVanillaOrder::Standing(StandingVariants::Partial(o)) => o,
        _ => unreachable!("builder must produce partial standing order")
    };

    // Explicitly ensure max_gas < min_qty
    order.max_extra_fee_asset0 = 100;
    order.min_amount_in = 200;

    GroupedVanillaOrder::Standing(StandingVariants::Partial(order))
}

#[test]
fn test_valid_order_passes_all_validators() {
    let order = make_base_order();

    for validator in ORDER_VALIDATORS {
        let mut state = OrderValidationState::new(&order);
        let result = validator.validate_order(&mut state);
        assert_eq!(result, Ok(()), "Validator {:?} unexpectedly failed", validator);
    }
}
