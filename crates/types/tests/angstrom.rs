// Tests around building the Angstrom bundle

mod solutionlib;

use angstrom_types::{
    contract_payloads::{angstrom::AngstromBundle, asset::builder::AssetBuilder},
    matching::uniswap::{Direction, PoolPriceVec, PoolSnapshot, Quantity},
    orders::PoolSolution
};
use base64::Engine;
use solutionlib::{BAD_USER_ORDERS, FROM_ABOVE, FROM_WILL, SETTLE_SAVE, TICK_SPACING, WEIRD_SWAP};
use tracing::Level;

pub fn with_tracing<T>(f: impl FnOnce() -> T) -> T {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .with_line_number(true)
        .with_file(true)
        .finish();
    tracing::subscriber::with_default(subscriber, f)
}

#[test]
fn build_bundle() {
    with_tracing(|| {
        let bytes = base64::prelude::BASE64_STANDARD
            .decode(BAD_USER_ORDERS)
            .unwrap();
        let (solution, orders_by_pool, snapshot, t0, t1, store_index, shared_gas): (
            PoolSolution,
            _,
            PoolSnapshot,
            _,
            _,
            _,
            _
        ) = serde_json::from_slice(&bytes).unwrap();

        let vec_one = (snapshot.current_price() + Quantity::Token0(663457929968124)).unwrap();
        let vec_two = (vec_one.end_bound.clone() + Quantity::Token1(17509420022687840846)).unwrap();
        // Vec by pulling T0 instead of adding T1
        // let vec_direct = (snapshot.current_price() -
        // Quantity::Token0(147398421527330)).unwrap();
        let vec_direct =
            (snapshot.current_price() + Quantity::Token1(3192774615752599438)).unwrap();

        println!(
            "Vec_one:  t0 - {} t1 - {} final_price - {:?}",
            vec_one.d_t0,
            vec_one.d_t1,
            vec_one.end_bound.price()
        );
        println!(
            "Vec_two:  t0 - {} t1 - {} final_price - {:?}",
            vec_two.d_t0,
            vec_two.d_t1,
            vec_two.end_bound.price()
        );
        println!(
            "Vec_dir:  t0 - {} t1 - {} final_price - {:?}",
            vec_direct.d_t0,
            vec_direct.d_t1,
            vec_direct.end_bound.price()
        );

        let mut top_of_block_orders = Vec::new();
        let mut pool_updates = Vec::new();
        let mut pairs = Vec::new();
        let mut user_orders = Vec::new();
        let mut asset_builder = AssetBuilder::new();

        AngstromBundle::process_solution(
            &mut pairs,
            &mut asset_builder,
            &mut user_orders,
            &orders_by_pool,
            &mut top_of_block_orders,
            &mut pool_updates,
            &solution,
            &snapshot,
            t0,
            t1,
            store_index,
            shared_gas
        )
        .expect("Bundle processing failed");

        let bundle = AngstromBundle::new(
            asset_builder.get_asset_array(),
            pairs,
            pool_updates,
            top_of_block_orders,
            user_orders
        );
        let (direction, quantity) = if bundle.pool_updates[0].zero_for_one {
            (Direction::SellingT0, Quantity::Token0(bundle.pool_updates[0].swap_in_quantity))
        } else {
            (Direction::BuyingT0, Quantity::Token1(bundle.pool_updates[0].swap_in_quantity))
        };
        tracing::trace!(?direction, ?quantity, "Got values from bundle");
        let inspect_vec =
            PoolPriceVec::from_swap(snapshot.current_price(), direction, quantity).unwrap();
        println!(
            "Final price: {:#?} {}",
            inspect_vec.end_bound.price(),
            inspect_vec.end_bound.tick()
        );
        println!("Bundle: {:#?}", bundle);
        tracing::trace!(start_price = ?inspect_vec.start_bound.price(), end_price = ?inspect_vec.end_bound.price(), inspect_vec.d_t0, inspect_vec.d_t1, "Vec inspect");
        println!("Number of swaps: {}", inspect_vec.steps.as_ref().unwrap().len());
        println!("Number of updates: {}", bundle.pool_updates[0].rewards_update.quantities().len());
    })
}
