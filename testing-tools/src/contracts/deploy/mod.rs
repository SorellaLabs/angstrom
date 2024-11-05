use alloy::primitives::{address, keccak256, Address, Bytes, B256, U160, U256};

pub mod angstrom;
pub mod mockreward;
pub mod tokens;
pub mod uniswap_flags;

const DEFAULT_CREATE2_FACTORY: Address = address!("4e59b44847b379578588920cA78FbF26c0B4956C");

/// Attempt to find a target address that includes the appropriate flags
/// Returns the address found and the salt needed to pad the initcode to
/// deploy to that address
pub fn mine_address(flags: U160, mask: U160, initcode: &Bytes) -> (Address, U256) {
    mine_address_with_factory(DEFAULT_CREATE2_FACTORY, flags, mask, initcode)
}

pub fn mine_address_with_factory(
    factory: Address,
    flags: U160,
    mask: U160,
    initcode: &Bytes
) -> (Address, U256) {
    let init_code_hash = keccak256(initcode);
    let mut salt = U256::ZERO;
    let mut counter: u128 = 0;
    loop {
        let target_address: Address = factory.create2(B256::from(salt), init_code_hash);
        let u_address: U160 = target_address.into();
        if (u_address & mask) == flags {
            break;
        }
        salt += U256::from(1_u8);
        counter += 1;
        if counter > 100_000 {
            panic!("We tried this too many times!")
        }
    }
    let final_address = factory.create2(B256::from(salt), init_code_hash);
    (final_address, salt)
}
