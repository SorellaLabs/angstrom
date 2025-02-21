use alloy::primitives::{address, fixed_bytes, uint, Address, B256, U160, U256};

pub mod angstrom;
pub mod tokens;
pub mod uniswap_flags;

use uniswap_flags::UniswapFlags;

/// Attempt to find a target address that includes the appropriate flags
/// Returns the address found and the salt needed to pad the initcode to
/// deploy to that address
pub fn mine_create3_address(owner: Address) -> (Address, U256, u8) {
    let mut salt = U256::from(Into::<U160>::into(owner));
    let nonce = 0u8;
    salt <<= 96;
    let mut addr;
    loop {
        addr = sub_zero_create3(salt.into(), nonce);
        if angstrom_addr_valid(addr) {
            break;
        }
        salt += uint!(1U256);
    }
    (addr, salt, nonce)
}

pub fn mine_create3_address_uni(owner: Address) -> (Address, U256, u8) {
    let mut salt = U256::from(Into::<U160>::into(owner));
    let nonce = 0u8;
    salt <<= 96;
    salt += uint!(69420U256);
    let addr = sub_zero_create3(salt.into(), nonce);
    (addr, salt, nonce)
}

pub const SUB_ZERO_FACTORY: Address = address!("000000000000b361194cfe6312ee3210d53c15aa");
const DEPLOY_PROXY_INITHASH: B256 =
    fixed_bytes!("1decbcf04b355d500cbc3bd83c892545b4df34bd5b2c9d91b9f7f8165e2095c3");

fn angstrom_addr_valid(addr: Address) -> bool {
    use UniswapFlags::*;
    if !has_permissions(addr, BeforeInitialize | AfterInitialize) {
        return false;
    }
    if !has_permissions(addr, BeforeAddLiquidity | BeforeRemoveLiquidity) {
        return false;
    }
    if has_any_permission(addr, AfterAddLiquidity | AfterRemoveLiquidity | AfterSwap) {
        return false;
    }
    if !has_permission(addr, BeforeSwap) {
        return false;
    }

    hook_addr_valid(addr)
}

/// Assumes hook with fee of **0**.
fn hook_addr_valid(addr: Address) -> bool {
    use UniswapFlags::*;
    if !has_permission(addr, BeforeSwap) && has_permission(addr, BeforeSwapReturnsDelta) {
        return false;
    }
    if !has_permission(addr, AfterSwap) && has_permission(addr, AfterSwapReturnsDelta) {
        return false;
    }
    if !has_permission(addr, AfterRemoveLiquidity)
        && has_permission(addr, AfterRemoveLiquidityReturnsDelta)
    {
        return false;
    }
    if !has_permission(addr, AfterAddLiquidity)
        && has_permission(addr, AfterAddLiquidityReturnsDelta)
    {
        return false;
    }

    let bits: U160 = addr.into();

    // Has at least some flag
    bits & UniswapFlags::mask() > U160::from(0)
}

fn has_permission(addr: Address, f: UniswapFlags) -> bool {
    let bits: U160 = addr.into();
    bits & f.flag() == f.flag()
}

fn has_permissions(addr: Address, flags: U160) -> bool {
    let bits: U160 = addr.into();
    bits & flags == flags
}

fn has_any_permission(addr: Address, flags: U160) -> bool {
    let bits: U160 = addr.into();
    bits & flags != U160::from(0)
}

fn sub_zero_create3(salt: B256, nonce: u8) -> Address {
    let deploy_proxy = SUB_ZERO_FACTORY.create2(salt, DEPLOY_PROXY_INITHASH);
    deploy_proxy.create((nonce as u64).wrapping_add(1))
}
