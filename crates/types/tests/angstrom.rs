// Tests around building the Angstrom bundle

mod solutionlib;

use angstrom_types::{
    contract_payloads::{angstrom::AngstromBundle, asset::builder::AssetBuilder},
    orders::PoolSolution,
    traits::BundleProcessing,
    uni_structure::BaselinePoolState
};
use base64::Engine;
use solutionlib::ANOTHER_BAD;
use tracing_subscriber::EnvFilter;

pub fn with_tracing<T>(f: impl FnOnce() -> T) -> T {
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_line_number(true)
        .with_file(true)
        .finish();
    tracing::subscriber::with_default(subscriber, f)
}

#[ignore]
#[test]
fn build_bundle() {
    with_tracing(|| {
        let bytes = base64::prelude::BASE64_STANDARD
            .decode(ANOTHER_BAD)
            .unwrap();
        let (solution, orders_by_pool, snapshot, t0, t1, store_index, shared_gas): (
            PoolSolution,
            _,
            BaselinePoolState,
            _,
            _,
            _,
            _
        ) = serde_json::from_slice(&bytes).unwrap();

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
        println!("Bundle: {:#?}", bundle.pool_updates);
    })
}
