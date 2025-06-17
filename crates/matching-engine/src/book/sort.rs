use angstrom_types::sol_bindings::RawPoolOrder;

use super::BookOrder;

// There are lots of different ways we can sort the orders we get in, so let's
// make this modular

pub enum SortStrategy {
    Unsorted,
    ByPriceByVolume,
    PricePartialVolume
}

impl Default for SortStrategy {
    fn default() -> Self {
        Self::Unsorted
    }
}

impl SortStrategy {
    pub fn sort_bids(&self, bids: &mut [BookOrder]) {
        match self {
            Self::Unsorted => (),
            // First sort by price, then put partial orders before exact orders then sort by volume,
            // gas, gas_units
            Self::PricePartialVolume => bids.sort_unstable(),
            // First sort by price, then volume, gas, gas_units
            Self::ByPriceByVolume => bids.sort_unstable()
        }
    }

    pub fn sort_asks(&self, asks: &mut [BookOrder]) {
        match self {
            Self::Unsorted => (),
            // First sort by price, then put partial orders before exact orders then sort by volume,
            // gas, gas_units
            Self::PricePartialVolume => asks.sort_unstable(),
            // First sort by price, then volume, gas, gas_units
            Self::ByPriceByVolume => asks.sort_unstable()
        }
    }
}
