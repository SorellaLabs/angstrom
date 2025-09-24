mod implementations;

mod primitives;

use rand::{Rng, RngCore, distr::StandardUniform, prelude::Distribution};

// need to redefine the Random trait due to trait + types (reth) not being ours
pub trait Randomizer<T>: Rng {
    fn generate(&mut self) -> T;

    fn gen_many(&mut self, count: usize) -> Vec<T> {
        (0..count).map(|_| Randomizer::generate(self)).collect()
    }
}

impl<T, R> Randomizer<T> for R
where
    StandardUniform: Distribution<T>,
    R: RngCore
{
    fn generate(&mut self) -> T {
        self.random()
    }
}

pub trait RandomizerSized<T>: Rng {
    fn gen_sized<const SIZE: usize>(&mut self) -> T;

    fn gen_many_sized<const SIZE: usize>(&mut self, count: usize) -> Vec<T> {
        (0..count).map(|_| self.gen_sized::<SIZE>()).collect()
    }
}

pub trait RandomValues
where
    StandardUniform: Distribution<Self>,
    Self: Sized
{
    fn generate() -> Self {
        let mut rng = rand::rng();
        rng.random()
    }

    fn gen_many(count: usize) -> Vec<Self> {
        let mut rng = rand::rng();
        rng.gen_many(count)
    }
}

impl<T> RandomValues for T
where
    StandardUniform: Distribution<T>,
    T: Sized
{
}
