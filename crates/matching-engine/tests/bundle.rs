use base64::Engine;
use matching_engine::{book::OrderBook, matcher::VolumeFillMatcher};

mod booklib;
use booklib::{AMM_SIDE_BOOK, BAD_POOL, DEBT_WRONG_SIDE, GOOD_BOOK, MATH_ZERO, ZERO_ASK_BOOK};
use tracing::Level;

pub fn with_tracing<T>(f: impl FnOnce() -> T) -> T {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_line_number(true)
        .with_file(true)
        .finish();
    tracing::subscriber::with_default(subscriber, f)
}

#[test]
#[ignore]
fn check_all_existing_books() {
    for input in [ZERO_ASK_BOOK, MATH_ZERO, GOOD_BOOK, AMM_SIDE_BOOK, DEBT_WRONG_SIDE] {
        let bytes = base64::prelude::BASE64_STANDARD.decode(input).unwrap();
        let book: OrderBook = serde_json::from_slice(&bytes).unwrap();
        let mut matcher = VolumeFillMatcher::new(&book);
        let solve = matcher.run_match();
        let solution = matcher.from_checkpoint().unwrap().solution(None);
        println!("EndReason: {:?}", solve);
        println!("Solution: {:?}", solution);
    }
}

#[test]
#[ignore]
fn build_and_ship_random_bundle() {
    with_tracing(|| {
        let bytes = base64::prelude::BASE64_STANDARD.decode(BAD_POOL).unwrap();
        let book: OrderBook = serde_json::from_slice(&bytes).unwrap();
        println!("Book: {:?}", book);
        let mut matcher = VolumeFillMatcher::new(&book);
        let solve = matcher.run_match();
        let solution = matcher.from_checkpoint().unwrap().solution(None);
        println!("EndReason: {:?}", solve);
        println!("Solution: {:?}", solution);
    });
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
