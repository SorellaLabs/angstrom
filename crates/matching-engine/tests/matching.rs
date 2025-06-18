use std::collections::HashMap;

use alloy::primitives::U256;
use alloy_primitives::{FixedBytes, I256};
use angstrom_types::{
    matching::{Ray, get_quantities_at_price},
    orders::OrderFillState,
    sol_bindings::RawPoolOrder
};
use matching_engine::{
    book::{BookOrder, OrderBook},
    matcher::delta::{DeltaMatcher, DeltaMatcherToB}
};
use testing_tools::type_generator::orders::UserOrderBuilder;
use tracing::Level;

pub fn with_tracing<T>(f: impl FnOnce() -> T) -> T {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .with_line_number(true)
        .with_file(true)
        .finish();
    tracing::subscriber::with_default(subscriber, f)
}

struct TestOrder {
    q: u128,
    p: Ray
}

impl TestOrder {
    fn exact_bid(q: u128, p: Ray) -> BookOrder {
        Self { q, p }.to_order(true)
    }

    fn partial_bid(q: u128, p: Ray) -> BookOrder {
        Self { q, p }.to_partial_order(true)
    }

    fn exact_ask(q: u128, p: Ray) -> BookOrder {
        Self { q, p }.to_order(false)
    }

    fn partial_ask(q: u128, p: Ray) -> BookOrder {
        Self { q, p }.to_partial_order(false)
    }
}

impl TestOrder {
    pub fn to_order(&self, is_bid: bool) -> BookOrder {
        let min_price = if is_bid { self.p.inv_ray_round(true) } else { self.p };
        UserOrderBuilder::new()
            .amount(self.q)
            .min_price(min_price)
            .exact()
            .exact_in(!is_bid)
            .is_bid(is_bid)
            .with_storage()
            .is_bid(is_bid)
            .build()
    }

    pub fn to_partial_order(&self, is_bid: bool) -> BookOrder {
        let min_price = if is_bid { self.p.inv_ray_round(true) } else { self.p };
        UserOrderBuilder::new()
            .amount(self.q)
            .min_price(min_price)
            .partial()
            .is_bid(is_bid)
            .with_storage()
            .is_bid(is_bid)
            .build()
    }
}

fn raw_price(p: u128) -> Ray {
    Ray::from(U256::from(p))
}

#[test]
fn delta_matcher_test() {
    with_tracing(|| {
        let orders = [
            TestOrder::exact_bid(5000, raw_price(20000000000000000000000000000000_u128)),
            TestOrder::partial_bid(50000, raw_price(20000000000000000000000000000000_u128)),
            TestOrder::partial_ask(1000, raw_price(10000000000000000000000000000000_u128)),
            TestOrder::partial_ask(10000, raw_price(10000000000000000000000000000000_u128))
        ];
        let book = OrderBook::new(
            FixedBytes::random(),
            None,
            vec![orders[0].clone(), orders[1].clone()],
            vec![orders[2].clone(), orders[3].clone()],
            Some(matching_engine::book::sort::SortStrategy::ByPriceByVolume)
        );
        println!("{:#?}", book);
        let mut matcher = DeltaMatcher::new(&book, DeltaMatcherToB::None);
        let solution = matcher.solution(None);
        println!("{:?}", solution);
        assert_eq!(solution.ucp, book.asks()[0].price(), "Price is not at partial fill Ask price");
        // Because it's a partial fill, our price should end at our ask price
        // And our total T0 state should sum to zero
        let mut total_t1 = I256::ZERO;
        for (order, outcome) in orders.iter().zip(solution.limit.iter()) {
            if outcome.id != order.order_id {
                panic!("Mismatched iteration, fix this test");
            }
            match outcome.outcome {
                OrderFillState::Unfilled | OrderFillState::Killed => continue,
                OrderFillState::CompleteFill => {
                    let (t1_net, ..) = get_quantities_at_price(
                        order.is_bid,
                        order.exact_in(),
                        order.amount(),
                        0,
                        0,
                        solution.ucp
                    );
                    let signed_t1 = if order.is_bid {
                        I256::unchecked_from(t1_net)
                    } else {
                        I256::unchecked_from(t1_net).saturating_neg()
                    };
                    total_t1 += signed_t1;
                }
                OrderFillState::PartialFill(q) => {
                    let signed_t1_filled = if order.is_bid {
                        I256::unchecked_from(solution.ucp.inverse_quantity(q, false))
                    } else {
                        I256::unchecked_from(q).saturating_neg()
                    };
                    total_t1 += signed_t1_filled
                }
            }
        }
        assert_eq!(total_t1, I256::ZERO, "T1 exchanged did not sum to zero");
    });
}

#[test]
fn delta_matcher_kill_order_test() {
    with_tracing(|| {
        let orders = [
            TestOrder::exact_bid(5000, raw_price(20000000000000000000000000000000_u128)),
            TestOrder::partial_bid(50000, raw_price(20000000000000000000000000000000_u128)),
            TestOrder::partial_ask(1000, raw_price(10000000000000000000000000000000_u128)),
            TestOrder::exact_ask(
                50000000000000000000,
                raw_price(10000000000000000000000000000000_u128)
            ),
            TestOrder::partial_ask(10000, raw_price(10000000000000000000000000000000_u128))
        ];
        let book = OrderBook::new(
            FixedBytes::random(),
            None,
            vec![orders[0].clone(), orders[1].clone()],
            vec![orders[2].clone(), orders[3].clone(), orders[4].clone()],
            Some(matching_engine::book::sort::SortStrategy::ByPriceByVolume)
        );
        println!("{:#?}", book);
        let mut matcher = DeltaMatcher::new(&book, DeltaMatcherToB::None);
        let solution = matcher.solution(None);
        println!("{:?}", solution);
        // Because it's a partial fill, our price should end at our ask price
        assert_eq!(solution.ucp, book.asks()[0].price(), "Price is not at partial fill Ask price");
        // And our total T0 state should sum to zero
        let mut total_t0 = I256::ZERO;
        let grouped_orders: HashMap<_, _> = orders.iter().map(|o| (o.order_id, o)).collect();
        for outcome in solution.limit.iter() {
            let Some(order) = grouped_orders.get(&outcome.id) else {
                panic!("Mismatched iteration, fix this test");
            };
            match outcome.outcome {
                OrderFillState::Unfilled | OrderFillState::Killed => continue,
                OrderFillState::CompleteFill => {
                    let (t1_net, ..) = get_quantities_at_price(
                        order.is_bid,
                        order.exact_in(),
                        order.amount(),
                        0,
                        0,
                        solution.ucp
                    );
                    let signed_t1 = if order.is_bid {
                        I256::unchecked_from(t1_net)
                    } else {
                        I256::unchecked_from(t1_net).saturating_neg()
                    };
                    total_t0 += signed_t1;
                }
                OrderFillState::PartialFill(q) => {
                    let signed_t1_filled = if order.is_bid {
                        I256::unchecked_from(solution.ucp.inverse_quantity(q, false))
                    } else {
                        I256::unchecked_from(q).saturating_neg()
                    };
                    total_t0 += signed_t1_filled
                }
            }
        }
        assert_eq!(total_t0, I256::ZERO, "T1 exchanged did not sum to zero");
    });
}
