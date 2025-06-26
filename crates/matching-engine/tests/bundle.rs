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
