use base64::Engine;
use matching_engine::{
    book::OrderBook,
    matcher::{
        VolumeFillMatcher,
        delta::{DeltaMatcher, DeltaMatcherToB}
    }
};

mod booklib;
use booklib::{
    AMM_SIDE_BOOK, DEBT_WRONG_SIDE, DELTA_BOOK_TEST, GOOD_BOOK, MATH_ZERO, WEIRD_BOOK,
    ZERO_ASK_BOOK
};
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
        let bytes = base64::prelude::BASE64_STANDARD.decode(WEIRD_BOOK).unwrap();
        let book: OrderBook = serde_json::from_slice(&bytes).unwrap();
        // println!("Book: {:#?}", book);
        let mut matcher = VolumeFillMatcher::new(&book);
        let solve = matcher.run_match();
        let solution = matcher.from_checkpoint().unwrap().solution(None);
        println!("EndReason: {:?}", solve);
        println!("Solution: {:#?}", solution.amm_quantity);
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

#[test]
#[ignore]
fn delta_matcher_test() {
    with_tracing(|| {
        let bytes = base64::prelude::BASE64_STANDARD
            .decode(DELTA_BOOK_TEST)
            .unwrap();
        let (book, tob, solve_for_t0): (OrderBook, DeltaMatcherToB, bool) =
            serde_json::from_slice(&bytes).unwrap();
        println!("TOB: {tob:#?}");
        let _amm = book.amm().unwrap();
        let mut matcher = DeltaMatcher::new(&book, tob, solve_for_t0);
        let _solution = matcher.solution(None);
        // let end_price = amm.at_price(solution.ucp.into(), true).unwrap();
        // let mid_map = PoolPriceVec::from_price_range(amm.current_price(true),
        // end_price).unwrap(); println!("Mid-map: {} {} {:?}",
        // mid_map.d_t0, mid_map.d_t1, mid_map.end_bound.as_ray());
        // println!("{:#?}", solution);
    })
}
