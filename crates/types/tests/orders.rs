use alloy_primitives::{Address, U256};
use angstrom_types::{matching::Ray, sol_bindings::RawPoolOrder};
use testing_tools::type_generator::orders::UserOrderBuilder;

#[test]
#[ignore]
fn handles_inverse_quantities() {
    let asset0 = Address::random();
    let asset1 = Address::random();
    let (t0, t1) = if asset0 < asset1 { (asset0, asset1) } else { (asset1, asset0) };
    let order = UserOrderBuilder::new()
        .asset_in(t0)
        .asset_out(t1)
        .ask()
        .is_exact(true)
        .exact_in(false)
        .amount(1000)
        .min_price(Ray::scale_to_ray(U256::from(100)))
        .build();
    assert!(!order.is_bid(), "Order defined as an ask but registers itself as a bid");
    // For an ask order that wants 1000 out at a price of 100 t1 / 1 t0, we should
    // have a quantity of 10 t0 we can offer to the market
    println!("Q: {}", order.quantity_t0());
    assert!(order.quantity_t0() == 10, "Incorrect quantity reported for ask order");

    let order = UserOrderBuilder::new()
        .asset_in(t1)
        .asset_out(t0)
        .bid()
        .is_exact(true)
        .exact_in(true)
        .amount(1000)
        .min_price(Ray::scale_to_ray(U256::from(100)).inv_ray())
        .build();
    assert!(order.is_bid(), "Order defined as a bid but registers itself as an ask");
    // For a bid order that wants to put 1000 in at a price of 1 t0 / 100 t1, we
    // should have a quantity of 10 t0 we can buy from the market
    assert!(order.quantity_t0() == 10, "Incorrect quantity reported for bid order");
}

#[test]
#[ignore]
fn rounds_inverse_quantities_properly() {
    let asset0 = Address::random();
    let asset1 = Address::random();
    let (t0, t1) = if asset0 < asset1 { (asset0, asset1) } else { (asset1, asset0) };
    let order = UserOrderBuilder::new()
        .asset_in(t0)
        .asset_out(t1)
        .is_exact(true)
        .exact_in(false)
        .amount(999)
        .min_price(Ray::scale_to_ray(U256::from(100)))
        .build();
    assert!(!order.is_bid(), "Order defined as an ask but registers itself as a bid");
    // For an ask order that wants 999 out at a price of 100 t1 / 1 t0, we should
    // have a quantity of 10 t0 we can offer to the market as we should round up
    assert!(order.quantity_t0() == 10, "Incorrect quantity reported");

    let order = UserOrderBuilder::new()
        .asset_in(t0)
        .asset_out(t1)
        .is_exact(true)
        .exact_in(false)
        .amount(1001)
        .min_price(Ray::scale_to_ray(U256::from(100)))
        .build();
    assert!(!order.is_bid(), "Order defined as an ask but registers itself as a bid");
    // For an ask order that wants 1001 out at a price of 100 t1 / 1 t0, we should
    // have a quantity of 11 t0 we can offer to the market as we should round up
    assert!(order.quantity_t0() == 11, "Incorrect quantity reported");

    let order = UserOrderBuilder::new()
        .asset_in(t1)
        .asset_out(t0)
        .is_exact(true)
        .exact_in(true)
        .amount(999)
        .min_price(Ray::scale_to_ray(U256::from(100)).inv_ray())
        .build();
    assert!(order.is_bid(), "Order defined as a bid but registers itself as an ask");
    // For a bid order that wants to put 999 in at a price of 1 t0 / 100 t1, we
    // should have a quantity of 9 t0 we can buy from the market
    assert!(order.quantity_t0() == 9, "Incorrect quantity reported");

    let order = UserOrderBuilder::new()
        .asset_in(t1)
        .asset_out(t0)
        .is_exact(true)
        .exact_in(true)
        .amount(1001)
        .min_price(Ray::scale_to_ray(U256::from(100)).inv_ray())
        .build();
    assert!(order.is_bid(), "Order defined as a bid but registers itself as an ask");
    // For a bid order that wants to put 1001 in at a price of 1 t0 / 100 t1, we
    // should have a quantity of 10 t0 we can buy from the market
    assert!(order.quantity_t0() == 10, "Incorrect quantity reported");
}
