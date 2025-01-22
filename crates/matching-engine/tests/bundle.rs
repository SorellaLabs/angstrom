use base64::Engine;
use matching_engine::{book::OrderBook, matcher::VolumeFillMatcher};

mod booklib;
use booklib::AMM_SIDE_BOOK;

#[test]
#[ignore]
fn build_and_ship_random_bundle() {
    let bytes = base64::prelude::BASE64_STANDARD
        .decode(AMM_SIDE_BOOK)
        .unwrap();
    let book: OrderBook = serde_json::from_slice(&bytes).unwrap();
    println!("Book: {:?}", book);
    let mut matcher = VolumeFillMatcher::new(&book);
    let solve = matcher.run_match();
    let solution = matcher.from_checkpoint().unwrap().solution(None);
    println!("EndReason: {:?}", solve);
    println!("Solution: {:?}", solution);
    // Will migrate things to here later, right now we have other tests to work
    // on

    // let proposal = ProposalBuilder::new()
    //     .for_pools(pools)
    //     .for_random_pools(3)
    //     .for_block(10)
    //     .order_count(50)
    //     .preproposal_count(5)
    //     .build();
    // let _bundle = AngstromBundle::from_proposal(&proposal, gas_details,
    // pools).unwrap(); Contr
}
