mod implementations;

mod primitives;

use rand::{Rng, distributions::Standard, prelude::Distribution};

// need to redefine the Random trait due to trait + types (reth) not being ours
pub trait Randomizer<T>: Rng {
    fn generate(&mut self) -> T;

    fn gen_many(&mut self, count: usize) -> Vec<T> {
        (0..count).map(|_| Randomizer::generate(self)).collect()
    }
}

impl<T, R> Randomizer<T> for R
where
    Standard: Distribution<T>,
    R: Rng
{
    fn generate(&mut self) -> T {
        self.r#gen()
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
    Standard: Distribution<Self>,
    Self: Sized
{
    fn generate() -> Self {
        let mut rng = rand::thread_rng();
        Rng::r#gen(&mut rng)
    }

    fn gen_many(count: usize) -> Vec<Self> {
        let mut rng = rand::thread_rng();
        rng.gen_many(count)
    }
}

impl<T> RandomValues for T
where
    Standard: Distribution<T>,
    T: Sized
{
}
