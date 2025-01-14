/// Strategies are different ways we can look at a "solved" order book and
/// determine whether the solution is optimal or not.  As per internal
/// conversations on this topic, there are a lot of different heurestics and
/// algorithms we can use to determine our final clearing price and the orders
/// that will be filled at that price.
///
/// The intent is to implement several different strategies here and compare
/// them via a suite of tests that will help us determine what the optimal
/// matching strategy could be.
use crate::{book::OrderBook, matcher::VolumeFillMatcher};

mod simplecheckpoint;
pub use simplecheckpoint::SimpleCheckpointStrategy;

/// Basic trait to describe a matching strategy
pub trait MatchingStrategy<'a> {
    /// Utility function to run this strategy against an order book.  Does the
    /// book's standard fill operation and then attempts to run the provided
    /// `finalize()` method to do our "last mile" computation
    fn run(book: &'a OrderBook) -> Option<VolumeFillMatcher<'a>> {
        let mut solver = VolumeFillMatcher::new(book);
        solver.run_match();
        Self::finalize(solver)
    }

    /// Finalization function to make sure our book is in a valid state and, if
    /// not, do a "last mile" computation to get it there.  Will return
    /// `None` if the book is considered unsolveable.
    fn finalize(solver: VolumeFillMatcher) -> Option<VolumeFillMatcher>;
}
