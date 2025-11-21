use alloy_primitives::U256;
use angstrom_types_primitives::{
    contract_payloads::{
        Signature,
        angstrom::{OrderQuantities, StandingValidation, UserOrder}
    },
    sol_bindings::{
        RawPoolOrder,
        grouped_orders::{AllOrders, OrderWithStorageData}
    }
};

use crate::orders::OrderOutcome;

pub trait UserOrderFromInternal: Sized {
    fn from_internal_order(
        order: &OrderWithStorageData<AllOrders>,
        outcome: &OrderOutcome,
        _: U256,
        pair_index: u16
    ) -> eyre::Result<Self>;

    fn from_internal_order_max_gas(
        order: &OrderWithStorageData<AllOrders>,
        outcome: &OrderOutcome,
        pair_index: u16
    ) -> Self;
}

impl UserOrderFromInternal for UserOrder {
    fn from_internal_order(
        order: &OrderWithStorageData<AllOrders>,
        outcome: &OrderOutcome,
        _: U256,
        pair_index: u16
    ) -> eyre::Result<Self> {
        let (order_quantities, standing_validation, recipient) = match &order.order {
            AllOrders::PartialFlash(p_o) => (
                OrderQuantities::Partial {
                    min_quantity_in: p_o.min_amount_in,
                    max_quantity_in: p_o.max_amount_in,
                    filled_quantity: outcome.fill_amount(p_o.max_amount_in)
                },
                None,
                p_o.recipient
            ),
            AllOrders::PartialStanding(p_o) => (
                OrderQuantities::Partial {
                    min_quantity_in: p_o.min_amount_in,
                    max_quantity_in: p_o.max_amount_in,
                    filled_quantity: outcome.fill_amount(p_o.max_amount_in)
                },
                Some(StandingValidation::new(p_o.nonce, p_o.deadline.to())),
                p_o.recipient
            ),
            AllOrders::ExactFlash(o) => {
                (OrderQuantities::Exact { quantity: order.amount() }, None, o.recipient)
            }
            AllOrders::ExactStanding(o) => (
                OrderQuantities::Exact { quantity: order.amount() },
                Some(StandingValidation::new(o.nonce, o.deadline.to())),
                o.recipient
            ),
            AllOrders::TOB(_) => panic!("tob found in limit orders")
        };
        let hook_bytes = match &order.order {
            AllOrders::PartialFlash(p_o) => p_o.hook_data.clone(),
            AllOrders::PartialStanding(p_o) => p_o.hook_data.clone(),
            AllOrders::ExactFlash(o) => o.hook_data.clone(),
            AllOrders::ExactStanding(o) => o.hook_data.clone(),
            AllOrders::TOB(_) => panic!("tob found in limit orders")
        };
        let hook_data = if hook_bytes.is_empty() { None } else { Some(hook_bytes) };

        let recipient = (!recipient.is_zero()).then_some(recipient);
        let gas_used: u128 = (order.priority_data.gas).to();
        if gas_used > order.max_gas_token_0() {
            return Err(eyre::eyre!("order used more gas than allocated"));
        }

        let sig = order.order_signature().unwrap();
        let signature = Signature::from(sig);

        Ok(Self {
            ref_id: 0,
            use_internal: order.use_internal(),
            pair_index,
            min_price: order.limit_price(),
            recipient,
            hook_data,
            zero_for_one: !order.is_bid,
            standing_validation,
            order_quantities,
            max_extra_fee_asset0: order.max_gas_token_0(),
            extra_fee_asset0: gas_used,
            exact_in: order.exact_in(),
            signature
        })
    }

    fn from_internal_order_max_gas(
        order: &OrderWithStorageData<AllOrders>,
        outcome: &OrderOutcome,
        pair_index: u16
    ) -> Self {
        let (order_quantities, standing_validation, recipient) = match &order.order {
            AllOrders::PartialFlash(p_o) => (
                OrderQuantities::Partial {
                    min_quantity_in: p_o.min_amount_in,
                    max_quantity_in: p_o.max_amount_in,
                    filled_quantity: outcome.fill_amount(p_o.max_amount_in)
                },
                None,
                p_o.recipient
            ),
            AllOrders::PartialStanding(p_o) => (
                OrderQuantities::Partial {
                    min_quantity_in: p_o.min_amount_in,
                    max_quantity_in: p_o.max_amount_in,
                    filled_quantity: outcome.fill_amount(p_o.max_amount_in)
                },
                Some(StandingValidation::new(p_o.nonce, p_o.deadline.to())),
                p_o.recipient
            ),
            AllOrders::ExactFlash(o) => {
                (OrderQuantities::Exact { quantity: order.amount() }, None, o.recipient)
            }
            AllOrders::ExactStanding(o) => (
                OrderQuantities::Exact { quantity: order.amount() },
                Some(StandingValidation::new(o.nonce, o.deadline.to())),
                o.recipient
            ),
            AllOrders::TOB(_) => panic!("tob found in limit orders")
        };
        let hook_bytes = match &order.order {
            AllOrders::PartialFlash(p_o) => p_o.hook_data.clone(),
            AllOrders::PartialStanding(p_o) => p_o.hook_data.clone(),
            AllOrders::ExactFlash(o) => o.hook_data.clone(),
            AllOrders::ExactStanding(o) => o.hook_data.clone(),
            AllOrders::TOB(_) => panic!("tob found in limit orders")
        };
        let hook_data = if hook_bytes.is_empty() { None } else { Some(hook_bytes) };
        let sig = order.order_signature().unwrap();
        let signature = Signature::from(sig);

        let recipient = (!recipient.is_zero()).then_some(recipient);

        Self {
            ref_id: 0,
            use_internal: order.use_internal(),
            pair_index,
            min_price: order.limit_price(),
            recipient,
            hook_data,
            zero_for_one: !order.is_bid,
            standing_validation,
            order_quantities,
            max_extra_fee_asset0: order.max_gas_token_0(),
            extra_fee_asset0: order.priority_data.gas.to(),
            exact_in: order.exact_in(),
            signature
        }
    }
}
