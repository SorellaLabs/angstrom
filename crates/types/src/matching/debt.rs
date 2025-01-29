use std::ops::{Add, AddAssign, Sub};

use malachite::{
    num::{
        arithmetic::traits::{DivRound, FloorSqrt, Pow},
        conversion::traits::SaturatingFrom
    },
    rounding_modes::RoundingMode,
    Natural
};
use tracing::debug;

use super::{math::low_to_high, uniswap::PoolPrice, Ray};
use crate::matching::const_2_192;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DebtType {
    ExactIn(u128),
    ExactOut(u128)
}

impl DebtType {
    pub fn new(q: u128, is_bid: bool) -> Self {
        if is_bid {
            Self::ExactIn(q)
        } else {
            Self::ExactOut(q)
        }
    }

    pub fn exact_in(q: u128) -> Self {
        Self::ExactIn(q)
    }

    pub fn exact_out(q: u128) -> Self {
        Self::ExactOut(q)
    }

    /// Whether or not price calculations for this debt type should round up
    pub fn round_up(&self) -> bool {
        match self {
            Self::ExactIn(_) => false,
            Self::ExactOut(_) => true
        }
    }

    pub fn same_type(&self, q: u128) -> Self {
        match self {
            Self::ExactIn(_) => Self::ExactIn(q),
            Self::ExactOut(_) => Self::ExactOut(q)
        }
    }

    pub fn magnitude(&self) -> u128 {
        match self {
            Self::ExactIn(q) | Self::ExactOut(q) => *q
        }
    }

    pub fn is_empty(&self) -> bool {
        self.magnitude() == 0
    }

    pub fn t0_at_price<T: Into<Ray>>(&self, price: T) -> u128 {
        // If it's an ExactIn debt our output is rounded down, otherwise it's ExactOut
        // and the input is rounded up
        let round_up = match self {
            Self::ExactIn(_) => false,
            Self::ExactOut(_) => true
        };
        let ray_price: Ray = price.into();
        ray_price.inverse_quantity(self.magnitude(), round_up)
    }

    pub fn slack_at_price<T: Into<Ray>>(&self, price: T) -> u128 {
        let ray_price: Ray = price.into();
        // If I'm on the Ask side (ExactOut debt) I need to substract 1 from my slack
        let ask_side = match self {
            Self::ExactIn(_) => 0,
            Self::ExactOut(_) => 1
        };
        ray_price
            .inverse_remainder(self.magnitude())
            .saturating_sub(ask_side)
    }

    pub fn same_side(&self, other: &Self) -> bool {
        if let Self::ExactIn(_) = self {
            matches!(other, Self::ExactIn(_))
        } else {
            matches!(other, Self::ExactOut(_))
        }
    }
}

impl Add for DebtType {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            // Like types accumulate
            (Self::ExactIn(q_1), Self::ExactIn(q_2)) => Self::ExactIn(q_1 + q_2),
            (Self::ExactOut(q_1), Self::ExactOut(q_2)) => Self::ExactOut(q_1 + q_2),
            // Different types annihilate or maybe flip
            (Self::ExactIn(q_1), Self::ExactOut(q_2)) if q_2 > q_1 => Self::ExactOut(q_2 - q_1),
            (Self::ExactIn(q_1), Self::ExactOut(q_2)) => Self::ExactIn(q_1 - q_2),
            (Self::ExactOut(q_1), Self::ExactIn(q_2)) if q_2 > q_1 => Self::ExactIn(q_2 - q_1),
            (Self::ExactOut(q_1), Self::ExactIn(q_2)) => Self::ExactOut(q_1 - q_2)
        }
    }
}

impl Sub<u128> for DebtType {
    type Output = Self;

    fn sub(self, rhs: u128) -> Self::Output {
        match self {
            Self::ExactIn(q) => Self::ExactIn(q.saturating_sub(rhs)),
            Self::ExactOut(q) => Self::ExactOut(q.saturating_sub(rhs))
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Debt {
    cur_price: Ray,
    magnitude: DebtType
}

impl Debt {
    pub fn new<T: Into<Ray>>(magnitude: DebtType, price: T) -> Self {
        let cur_price: Ray = price.into();
        Self { cur_price, magnitude }
    }

    pub fn from_quantities(t0_q: u128, t1_q: u128, exact_in: bool) -> Self {
        let magnitude = if exact_in { DebtType::exact_in(t1_q) } else { DebtType::exact_out(t1_q) };
        let price = Ray::calc_price_generic(t0_q, t1_q, magnitude.round_up());
        Debt::new(magnitude, price)
    }

    /// Creates a new Debt item at the price provided with the same quantity as
    /// the current Debt
    pub fn set_price(&self, new_price: Ray) -> Self {
        Self { cur_price: new_price, ..*self }
    }

    pub fn magnitude(&self) -> u128 {
        self.magnitude.magnitude()
    }

    pub fn price(&self) -> Ray {
        self.cur_price
    }

    /// Returns the additional T0 needed to increase a debt from a specific T1
    /// to another greater T1 count
    pub fn additional_t0_needed(&self, added_t1: u128) -> u128 {
        let new_magnitude = self.magnitude + self.magnitude.same_type(added_t1);
        let new_debt = Self { magnitude: new_magnitude, cur_price: self.cur_price };
        new_debt.current_t0() - self.current_t0()
    }

    /// Does this debt exist on the bid side of the book?  A debt is on the bid
    /// side of the book when it's an ExactIn debt (i.e. I would like to pay 100
    /// T1 and get as many T0 as I can for that amount)
    pub fn bid_side(&self) -> bool {
        matches!(self.magnitude, DebtType::ExactIn(_))
    }

    /// Return the T1 quantity of debt "slack" - i.e. the amount that is being
    /// rounded away at the current price.  This is primarily for use in order
    /// quantity calculations to ensure we don't over or underfill orders
    pub fn slack(&self) -> u128 {
        self.magnitude.slack_at_price(self.cur_price)
    }

    /// Given the Debt's direction and rounding, return the low and high price
    /// that will result in an exchange for an identical amount of T0
    pub fn price_range(&self) -> (Ray, Ray) {
        let current_amount = self.current_t0();
        let bound_amount = match self.magnitude {
            DebtType::ExactIn(_) => current_amount.saturating_add(1),
            DebtType::ExactOut(_) => current_amount.saturating_sub(1)
        };
        debug!(current_amount, bound_amount, "Getting price range between targets");
        let (current_price, bound_price) = match (current_amount, bound_amount) {
            // If both values are zero, something is hecka wrong
            (0, 0) => (Ray::min_uniswap_price(), Ray::max_uniswap_price()),
            // If either value is 0, one of our bounds is max price
            (0, p) | (p, 0) => (
                Ray::max_uniswap_price(),
                Ray::calc_price_generic(p, self.magnitude(), self.magnitude.round_up())
            ),
            // Otherwise we can just do our normal math
            (c, b) => (
                Ray::calc_price_generic(c, self.magnitude(), self.magnitude.round_up()),
                Ray::calc_price_generic(b, self.magnitude(), self.magnitude.round_up())
            )
        };
        let (low, high) = low_to_high(&current_price, &bound_price);
        (*low, *high)
    }

    /// Will return true if the price provided is equivalent to the current
    /// price of this debt due to the rounding innate to performing the actual
    /// transaction
    pub fn valid_for_price(&self, price: Ray) -> bool {
        let (low, high) = self.price_range();
        debug!(low = ?low, high = ?high, "Validating for price range");

        match self.magnitude {
            DebtType::ExactIn(_) => price > low && price <= high,
            DebtType::ExactOut(_) => price >= low && price < high
        }
    }

    pub fn validate_and_set_price(&mut self, price: Ray) -> bool {
        let res = self.valid_for_price(price);
        if res {
            self.cur_price = price;
        }
        res
    }

    /// The current amount of T0 this debt is obligated to fill
    pub fn current_t0(&self) -> u128 {
        self.magnitude.t0_at_price(self.cur_price)
    }

    /// Returns the amount of T0 that needs to be reallocated for a given change
    /// in a debt's T1 value
    pub fn freed_t0(&self, t1_change: u128) -> u128 {
        let i_t0 = self.current_t0();
        // If we're freeing as much as is in the debt or more, we're freeing the whole
        // amount
        if t1_change >= self.magnitude() {
            return i_t0;
        }
        // Otherwise we figure out how much t0 we still need at the new magnitude and
        // return the difference
        let f_t0 = self
            .magnitude
            .same_type(self.magnitude.magnitude().saturating_sub(t1_change))
            .t0_at_price(self.cur_price);
        i_t0.saturating_sub(f_t0)
    }

    /// Returns the amount of T1 that needs to be reallocated for a given change
    /// in a debt's T0 value
    pub fn freed_t1(&self, t0_change: u128) -> u128 {
        // The amount of T1 we can free from this is equal to the quantity of T1 used at
        // this price plus our slack
        if t0_change >= self.current_t0() {
            self.magnitude()
        } else {
            let discriminant = if self.bid_side() { 1_u128 } else { 0_u128 };
            let target_amount = t0_change.saturating_add(discriminant);
            let round_up = self.bid_side();
            std::cmp::min(
                self.magnitude(),
                self.price()
                    .quantity(target_amount, round_up)
                    .saturating_sub(discriminant)
            )
        }
    }

    /// Create a new Debt object based on the price change created by filling
    /// the debt with a specified amount of t0
    pub fn partial_fill(&self, q: u128) -> Self {
        // P' = P + (y/q) = self.cur_price + Ray::calc_price(y, q)
        let (new_t0, current_t1) = match self.magnitude {
            DebtType::ExactIn(m) => {
                let t0 = self.current_t0() + q;
                (t0, m)
            }
            DebtType::ExactOut(m) => {
                let t0 = self.current_t0().saturating_sub(q);
                (t0, m)
            }
        };
        let new_price = Ray::calc_price_generic(new_t0, current_t1, self.magnitude.round_up());
        Self { magnitude: self.magnitude, cur_price: new_price }
    }

    /// Create a new Debt object based on the price change created by filling
    /// the debt with a specified amount of T1
    pub fn partial_fill_t1(&self, q: u128) -> Self {
        let current_t0 = self.current_t0();
        let new_t1 = self.magnitude - q;
        let new_price =
            Ray::calc_price_generic(current_t0, new_t1.magnitude(), self.magnitude.round_up());
        Self { magnitude: new_t1, cur_price: new_price }
    }

    /// Create a new debt object which just has our T1 reduced by the specified
    /// amount, the intention being that the T0 previously allocated has been
    /// elsewise pushed to a paired AMM in a Composite order
    pub fn flat_fill_t1(&self, q: u128) -> Self {
        let new_t1 = self.magnitude - q;
        Self { magnitude: new_t1, cur_price: self.cur_price }
    }

    /// Difference in t0 required to move the price from the current price to
    /// the target price
    pub fn dq_to_price(&self, target_price: &Ray) -> u128 {
        let final_t0 = self.magnitude.t0_at_price(*target_price);
        let current_t0: u128 = self.current_t0();
        if final_t0 > current_t0 {
            final_t0 - current_t0
        } else {
            current_t0 - final_t0
        }
    }

    /// Given an AMM liquidity and a quantity of t0 being bought to or sold from
    /// the AMM, returns the quantity that t0 will change to bring the price
    /// of this debt to the same price as the AMM
    pub fn calc_proportion(
        &self,
        amm_delta: u128,
        amm_liquidity: u128,
        amm_positive_delta: bool
    ) -> u128 {
        // Put our constants into Integer format
        let t1 = Natural::from(self.magnitude.magnitude());
        let t0_start = Natural::from(self.current_t0());
        let l = Natural::from(amm_liquidity);
        let dx = Natural::from(amm_delta);

        // Find our Sqrts that we're using, with extra precision baked in
        let sqrt_t1_x96 = ((&t1 << 192) as Natural).floor_sqrt();
        let sqrt_t0_start_x96 = ((&t0_start << 192) as Natural).floor_sqrt();

        let a_num_portion_1 = &dx * &sqrt_t1_x96;
        let a_num_portion_2 = &l * &sqrt_t0_start_x96;
        let a_numerator_sum = if amm_positive_delta {
            a_num_portion_1 + a_num_portion_2
        } else {
            a_num_portion_2 - a_num_portion_1
        };

        let (a_fraction, _) = (&a_numerator_sum).div_round(&l, RoundingMode::Nearest);
        debug!(
            numerator_sum = ?a_numerator_sum,
            denominator = ?l,
            result = ?&a_fraction,
            rounded = ?(&a_fraction >> 96),
            "Fraction calculation"
        );

        // if A = sqrt(x + dX) then we have to square A and subtract the original X
        let debt_delta_t0 = &t0_start
            - &a_fraction
                .pow(2)
                .div_round(const_2_192(), RoundingMode::Ceiling)
                .0;
        u128::saturating_from(&debt_delta_t0)
    }
}

impl Add<Debt> for Option<Debt> {
    type Output = Option<Debt>;

    fn add(self, rhs: Debt) -> Self::Output {
        match self {
            None => Some(rhs),
            Some(d) => d + rhs
        }
    }
}

impl AddAssign<Debt> for Option<Debt> {
    fn add_assign(&mut self, rhs: Debt) {
        *self = *self + rhs
    }
}

impl Add<Debt> for Debt {
    type Output = Option<Debt>;

    fn add(self, rhs: Debt) -> Self::Output {
        let magnitude = self.magnitude + rhs.magnitude;
        if magnitude.magnitude() == 0 {
            return None
        }
        // If our new magnitude is on the same side, we stay at our price.  If we flip,
        // we flip to the other price
        let cur_price =
            if magnitude.same_side(&self.magnitude) { self.cur_price } else { rhs.cur_price };
        Some(Self { magnitude, cur_price })
    }
}

impl<'a> Add<DebtType> for &'a Debt {
    type Output = Debt;

    fn add(self, rhs: DebtType) -> Self::Output {
        Debt { cur_price: self.cur_price, magnitude: self.magnitude + rhs }
    }
}

impl PartialEq<Ray> for Debt {
    fn eq(&self, other: &Ray) -> bool {
        self.valid_for_price(*other)
    }
}

impl PartialOrd<Ray> for Debt {
    fn partial_cmp(&self, other: &Ray) -> Option<std::cmp::Ordering> {
        if self == other {
            Some(std::cmp::Ordering::Equal)
        } else {
            Some(self.price().cmp(other))
        }
    }
}

impl<'a> PartialEq<PoolPrice<'a>> for Debt {
    fn eq(&self, other: &PoolPrice<'a>) -> bool {
        self.valid_for_price(other.as_ray())
    }
}

impl<'a> PartialOrd<PoolPrice<'a>> for Debt {
    fn partial_cmp(&self, other: &PoolPrice<'a>) -> Option<std::cmp::Ordering> {
        if self == other {
            Some(std::cmp::Ordering::Equal)
        } else {
            Some(self.price().cmp(&other.as_ray()))
        }
    }
}

#[cfg(test)]
mod test {
    use super::{Debt, DebtType};
    use crate::matching::Ray;

    #[test]
    fn test_debt_type_basics() {
        // Test creation methods
        let exact_in = DebtType::new(100, true);
        assert!(matches!(exact_in, DebtType::ExactIn(100)));

        let exact_out = DebtType::new(100, false);
        assert!(matches!(exact_out, DebtType::ExactOut(100)));

        // Test round_up behavior
        assert!(!exact_in.round_up());
        assert!(exact_out.round_up());

        // Test same_type
        assert!(matches!(exact_in.same_type(200), DebtType::ExactIn(200)));
        assert!(matches!(exact_out.same_type(200), DebtType::ExactOut(200)));

        // Test magnitude
        assert_eq!(exact_in.magnitude(), 100);
        assert_eq!(exact_out.magnitude(), 100);

        // Test is_empty
        assert!(!exact_in.is_empty());
        assert!(DebtType::exact_in(0).is_empty());

        // Test same_side
        assert!(exact_in.same_side(&DebtType::exact_in(200)));
        assert!(!exact_in.same_side(&DebtType::exact_out(200)));
    }

    #[test]
    fn test_debt_type_arithmetic() {
        // Test addition of same types
        let sum = DebtType::exact_in(100) + DebtType::exact_in(50);
        assert!(matches!(sum, DebtType::ExactIn(150)));

        // Test addition with cancellation
        let sum = DebtType::exact_in(100) + DebtType::exact_out(50);
        assert!(matches!(sum, DebtType::ExactIn(50)));

        let sum = DebtType::exact_in(50) + DebtType::exact_out(100);
        assert!(matches!(sum, DebtType::ExactOut(50)));

        // Test subtraction
        let diff = DebtType::exact_in(100) - 50;
        assert!(matches!(diff, DebtType::ExactIn(50)));

        // Test saturation
        let diff = DebtType::exact_in(50) - 100;
        assert!(matches!(diff, DebtType::ExactIn(0)));
    }

    #[test]
    fn debt_t0_magnitude_calculation() {
        let t0_q = 2214_u128;
        let t1_q = 55383699_u128;
        let price = Ray::calc_price_generic(t0_q, t1_q, false);
        let debt = Debt::new(DebtType::ExactIn(t1_q), price);
        assert!(debt.magnitude() == t1_q, "ExactIn Debt magnitude is not as initialized");
        assert!(
            debt.current_t0() == t0_q,
            "ExactIn Debt T0 fill is not as initialized: {} vs {}",
            debt.current_t0(),
            t0_q
        );

        let eo_price = Ray::calc_price_generic(t0_q, t1_q, true);
        let debt_out = Debt::new(DebtType::ExactOut(t1_q), eo_price);
        assert!(debt_out.magnitude() == t1_q, "ExactOut Debt magnitude is not as initialized");
        assert!(
            debt_out.current_t0() == t0_q,
            "ExactOut Debt T0 fill is not as initialized: {} vs {}",
            debt_out.current_t0(),
            t0_q
        );
    }

    #[test]
    fn test_debt_addition() {
        let price = Ray::calc_price_generic(100u64, 200u64, false);

        // Same type debts add
        let debt1 = Debt::new(DebtType::ExactIn(100), price);
        let debt2 = Debt::new(DebtType::ExactIn(50), price);
        let sum = debt1 + debt2;
        assert!(sum.is_some());
        assert_eq!(sum.unwrap().magnitude(), 150);

        // Opposite debts subtract
        let debt1 = Debt::new(DebtType::ExactIn(100), price);
        let debt2 = Debt::new(DebtType::ExactOut(50), price);
        let sum = debt1 + debt2;
        assert!(sum.is_some());
        assert_eq!(sum.unwrap().magnitude(), 50);

        // Equal opposite debts cancel to None
        let debt1 = Debt::new(DebtType::ExactIn(100), price);
        let debt2 = Debt::new(DebtType::ExactOut(100), price);
        let sum = debt1 + debt2;
        assert!(sum.is_none());
    }

    #[test]
    fn test_price_validation() {
        let price = Ray::calc_price_generic(100u64, 200u64, false);
        let mut debt = Debt::new(DebtType::ExactIn(200), price);

        // Test price range
        let (low, high) = debt.price_range();
        assert!(low < high);

        // Test valid prices
        assert!(debt.valid_for_price(price));
        assert!(debt.validate_and_set_price(price));
        assert_eq!(debt.price(), price);

        // Test invalid prices
        let invalid_price = Ray::calc_price_generic(100u64, 400u64, false); // Much different price
        assert!(!debt.valid_for_price(invalid_price));
        assert!(!debt.validate_and_set_price(invalid_price));
        assert_eq!(debt.price(), price); // Price should remain unchanged

        // Test ExactOut price validation
        let price_out = Ray::calc_price_generic(100u64, 200u64, true);
        let debt_out = Debt::new(DebtType::ExactOut(200), price_out);
        let (low_out, high_out) = debt_out.price_range();
        assert!(low_out < high_out);
        assert!(debt_out.valid_for_price(price_out));
    }

    #[test]
    fn test_debt_comparison() {
        let price1 = Ray::calc_price_generic(100u64, 200u64, false);
        let price2 = Ray::calc_price_generic(100u64, 300u64, false);
        let debt = Debt::new(DebtType::ExactIn(200), price1);

        // Test Ray comparisons
        assert!(debt == price1);
        assert!(debt < price2);
        assert!(debt > Ray::calc_price_generic(100u64, 400u64, false));

        // Test PoolPrice comparisons (would need to mock PoolPrice)
    }

    #[test]
    fn test_partial_fills() {
        let price = Ray::calc_price_generic(100u64, 200u64, false);
        let debt = Debt::new(DebtType::ExactIn(200), price);

        // Test partial fill
        let partial = debt.partial_fill(50);
        assert_eq!(partial.magnitude(), 200);
        assert!(partial.price() > debt.price());

        // Test partial fill T1
        let partial_t1 = debt.partial_fill_t1(50);
        assert_eq!(partial_t1.magnitude(), 150);

        // Test flat fill T1
        let flat = debt.flat_fill_t1(50);
        assert_eq!(flat.magnitude(), 150);
        assert_eq!(flat.price(), debt.price());
    }

    #[test]
    fn test_freed_amounts() {
        let price = Ray::calc_price_generic(100u64, 200u64, false);
        let debt = Debt::new(DebtType::ExactIn(200), price);

        // Test freed T0
        assert_eq!(debt.freed_t0(50), 25); // 25% of T1 frees 25% of T0
        assert_eq!(debt.freed_t0(200), 100); // Full amount
        assert_eq!(debt.freed_t0(300), 100); // Over amount still returns max

        // Test freed T1
        assert_eq!(debt.freed_t1(25), 50); // 25% of T0 frees 25% of T1
        assert_eq!(debt.freed_t1(100), 200); // Full amount
        assert_eq!(debt.freed_t1(150), 200); // Over amount still returns max
    }

    #[test]
    fn test_edge_cases() {
        // Test zero amounts
        let price = Ray::calc_price_generic(1u64, 1u64, false);
        let debt = Debt::new(DebtType::ExactIn(0), price);
        assert_eq!(debt.magnitude(), 0);
        assert_eq!(debt.current_t0(), 0);

        // Test max amounts
        let big_num = u128::MAX;
        let debt = Debt::new(DebtType::ExactIn(big_num), price);
        assert_eq!(debt.magnitude(), big_num);

        // Test price range with zero amounts
        let (low, high) = debt.price_range();
        assert!(low < high);
    }

    #[test]
    fn test_amm_proportion_calculation() {
        let price = Ray::calc_price_generic(100u64, 200u64, false);
        let debt = Debt::new(DebtType::ExactIn(200), price);

        // Test positive delta
        let proportion = debt.calc_proportion(50, 1000, true);
        assert!(proportion > 0);

        // Test negative delta
        let proportion = debt.calc_proportion(50, 1000, false);
        assert!(proportion > 0);

        // Test with zero liquidity should not panic
        let proportion = debt.calc_proportion(50, 0, true);
        assert_eq!(proportion, 0);
    }
}
