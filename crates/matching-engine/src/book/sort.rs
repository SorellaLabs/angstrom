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
            Self::PricePartialVolume => bids.sort_by(|a, b| {
                a.priority_data
                    .price
                    .cmp(&b.priority_data.price)
                    .then_with(|| a.is_partial().cmp(&b.is_partial()))
                    .then_with(|| a.priority_data.volume.cmp(&b.priority_data.volume))
                    .then_with(|| a.priority_data.gas.cmp(&b.priority_data.gas))
                    .then_with(|| a.priority_data.gas_units.cmp(&b.priority_data.gas_units))
            }),
            // First sort by price, then volume, gas, gas_units
            Self::ByPriceByVolume => bids.sort_by(|a, b| {
                let (ap, bp) = (&a.priority_data, &b.priority_data);
                ap.price
                    .cmp(&bp.price)
                    .then_with(|| ap.volume.cmp(&bp.volume))
                    .then_with(|| ap.gas.cmp(&bp.gas))
                    .then_with(|| ap.gas_units.cmp(&bp.gas_units))
            })
        }
    }

    pub fn sort_asks(&self, asks: &mut [BookOrder]) {
        match self {
            Self::Unsorted => (),
            // First sort by price, then put partial orders before exact orders then sort by volume,
            // gas, gas_units
            Self::PricePartialVolume => asks.sort_by(|a, b| {
                a.priority_data
                    .price
                    .cmp(&b.priority_data.price)
                    .then_with(|| a.is_partial().cmp(&b.is_partial()))
                    .then_with(|| a.priority_data.volume.cmp(&b.priority_data.volume))
                    .then_with(|| a.priority_data.gas.cmp(&b.priority_data.gas))
                    .then_with(|| a.priority_data.gas_units.cmp(&b.priority_data.gas_units))
            }),
            // First sort by price, then volume, gas, gas_units
            Self::ByPriceByVolume => asks.sort_by(|a, b| {
                let (ap, bp) = (&a.priority_data, &b.priority_data);
                ap.price
                    .cmp(&bp.price)
                    .then_with(|| ap.volume.cmp(&bp.volume))
                    .then_with(|| ap.gas.cmp(&bp.gas))
                    .then_with(|| ap.gas_units.cmp(&bp.gas_units))
            })
        }
    }
}
