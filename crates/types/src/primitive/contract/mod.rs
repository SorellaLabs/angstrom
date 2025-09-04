use std::{collections::HashMap, fmt::Debug, hash::Hash, sync::OnceLock};

use alloy::{
    dyn_abi::Eip712Domain,
    primitives::{Address, aliases::U24},
    sol
};
use alloy_primitives::{ChainId, address};

use crate::contract_bindings::angstrom::Angstrom::PoolKey;

sol! {
#![sol(all_derives = true)]
ERC20,
"src/primitive/contract/ERC20.json"}

pub use ERC20::*;

use crate::primitive::PoolId;

pub static ANGSTROM_ADDRESS: OnceLock<Address> = OnceLock::new();
pub static POSITION_MANAGER_ADDRESS: OnceLock<Address> = OnceLock::new();
pub static CONTROLLER_V1_ADDRESS: OnceLock<Address> = OnceLock::new();
pub static POOL_MANAGER_ADDRESS: OnceLock<Address> = OnceLock::new();
pub static GAS_TOKEN_ADDRESS: OnceLock<Address> = OnceLock::new();
pub static ANGSTROM_DEPLOYED_BLOCK: OnceLock<u64> = OnceLock::new();
pub static CHAIN_ID: OnceLock<u64> = OnceLock::new();
pub static ANGSTROM_DOMAIN: OnceLock<Eip712Domain> = OnceLock::new();

#[derive(Debug, Clone, Default)]
pub struct AngstromAddressBuilder {
    angstrom_address:         Address,
    position_manager_address: Address,
    controller_v1_address:    Address,
    pool_manager_address:     Address,
    gas_token_address:        Address,
    angstrom_deploy_block:    u64,
    chain_id:                 u64
}

impl AngstromAddressBuilder {
    pub fn with_angstrom_address(self, angstrom_address: Address) -> Self {
        Self { angstrom_address, ..self }
    }

    pub fn with_position_manager(self, position_manager_address: Address) -> Self {
        Self { position_manager_address, ..self }
    }

    pub fn with_controller(self, controller_v1_address: Address) -> Self {
        Self { controller_v1_address, ..self }
    }

    pub fn with_pool_manager(self, pool_manager_address: Address) -> Self {
        Self { pool_manager_address, ..self }
    }

    pub fn with_deploy_block(self, angstrom_deploy_block: u64) -> Self {
        Self { angstrom_deploy_block, ..self }
    }

    pub fn with_chain_id(self, chain_id: u64) -> Self {
        Self { chain_id, ..self }
    }

    pub fn with_gas_token(self, gas_token_address: Address) -> Self {
        Self { gas_token_address, ..self }
    }

    pub fn build(self) -> AngstromAddressConfig {
        AngstromAddressConfig {
            angstrom_address:         self.angstrom_address,
            position_manager_address: self.position_manager_address,
            controller_v1_address:    self.controller_v1_address,
            pool_manager_address:     self.pool_manager_address,
            angstrom_deploy_block:    self.angstrom_deploy_block,
            gas_token_address:        self.gas_token_address,
            chain_id:                 self.chain_id
        }
    }
}

/// used to set angstrom related setup args.
#[derive(Debug, Clone, Default)]
pub struct AngstromAddressConfig {
    angstrom_address:         Address,
    position_manager_address: Address,
    controller_v1_address:    Address,
    pool_manager_address:     Address,
    gas_token_address:        Address,
    angstrom_deploy_block:    u64,
    chain_id:                 u64
}

impl AngstromAddressConfig {
    pub const BASE_TESTNET: Self = Self {
        angstrom_address:         address!("0xc856DdFC924E9AeEaaFfB1905544b36470AC3ad4"),
        position_manager_address: address!("0xF967Ede45ED04ec89EcA04a4c7175b6E0106e3A8"),
        controller_v1_address:    address!("0xEd421745765bc1938848cAaB502ffF53c653ff13"),
        pool_manager_address:     address!("0x48bC5A530873DcF0b890aD50120e7ee5283E0112"),
        gas_token_address:        address!("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),

        angstrom_deploy_block: 0,
        chain_id:              1
    };
    pub const INTERNAL_TESTNET: Self = Self {
        angstrom_address:         address!("0xc856DdFC924E9AeEaaFfB1905544b36470AC3ad4"),
        position_manager_address: address!("0xF967Ede45ED04ec89EcA04a4c7175b6E0106e3A8"),
        controller_v1_address:    address!("0xEd421745765bc1938848cAaB502ffF53c653ff13"),
        pool_manager_address:     address!("0x48bC5A530873DcF0b890aD50120e7ee5283E0112"),
        gas_token_address:        address!("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),

        angstrom_deploy_block: 0,
        chain_id:              8453
    };

    /// Will panic if config has already been set
    pub fn init(self) {
        ANGSTROM_ADDRESS.set(self.angstrom_address).unwrap();
        POSITION_MANAGER_ADDRESS
            .set(self.position_manager_address)
            .unwrap();
        CONTROLLER_V1_ADDRESS
            .set(self.controller_v1_address)
            .unwrap();
        POOL_MANAGER_ADDRESS.set(self.pool_manager_address).unwrap();
        ANGSTROM_DEPLOYED_BLOCK
            .set(self.angstrom_deploy_block)
            .unwrap();
        CHAIN_ID.set(self.chain_id).unwrap();
        GAS_TOKEN_ADDRESS.set(self.gas_token_address).unwrap();
        ANGSTROM_DOMAIN
            .set(alloy::sol_types::eip712_domain!(
                name: "Angstrom",
                version: "v1",
                chain_id: self.chain_id,
                verifying_contract: self.angstrom_address,
            ))
            .unwrap();
    }

    pub fn try_init(self) {
        if self.gas_token_address != Address::ZERO {
            let _ = GAS_TOKEN_ADDRESS.set(self.gas_token_address);
        }
        if self.angstrom_address != Address::ZERO {
            let _ = ANGSTROM_ADDRESS.set(self.angstrom_address);
        }
        if self.position_manager_address != Address::ZERO {
            let _ = POSITION_MANAGER_ADDRESS.set(self.position_manager_address);
        }

        if self.controller_v1_address != Address::ZERO {
            let _ = CONTROLLER_V1_ADDRESS.set(self.controller_v1_address);
        }

        if self.pool_manager_address != Address::ZERO {
            let _ = POOL_MANAGER_ADDRESS.set(self.pool_manager_address);
        }

        if self.angstrom_deploy_block != 0 {
            let _ = ANGSTROM_DEPLOYED_BLOCK.set(self.angstrom_deploy_block);
        }

        if self.chain_id != 0 {
            let _ = CHAIN_ID.set(self.chain_id);
        }

        if self.chain_id != 0 && self.angstrom_address != Address::ZERO {
            let _ = ANGSTROM_DOMAIN.set(alloy::sol_types::eip712_domain!(
                name: "Angstrom",
                version: "v1",
                chain_id: self.chain_id,
                verifying_contract: self.angstrom_address,
            ));
        }
    }

    pub fn init_with_chain_fallback(self, chain_id: u64) {
        self.try_init();
        let _ = try_init_with_chain_id(chain_id);
    }
}

pub fn try_init_with_chain_id(chain_id: ChainId) -> eyre::Result<()> {
    let mut err = false;
    match chain_id {
        // Mainnet
        1 => {
            err |= ANGSTROM_ADDRESS
                .set(address!("0x0000000aa232009084Bd71A5797d089AA4Edfad4"))
                .is_err();
            err |= POSITION_MANAGER_ADDRESS
                .set(address!("0xbd216513d74c8cf14cf4747e6aaa6420ff64ee9e"))
                .is_err();
            err |= CONTROLLER_V1_ADDRESS
                .set(address!("0x1746484EA5e11C75e009252c102C8C33e0315fD4"))
                .is_err();
            err |= POOL_MANAGER_ADDRESS
                .set(address!("0x000000000004444c5dc75cB358380D2e3dE08A90"))
                .is_err();
            err |= CHAIN_ID.set(1).is_err();
            err |= ANGSTROM_DEPLOYED_BLOCK.set(22971781).is_err();
            err |= GAS_TOKEN_ADDRESS
                .set(address!("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"))
                .is_err();
            err |= ANGSTROM_DOMAIN
                .set(alloy::sol_types::eip712_domain!(
                    name: "Angstrom",
                    version: "v1",
                    chain_id: 1,
                    verifying_contract: address!("0x0000000aa232009084Bd71A5797d089AA4Edfad4"),
                ))
                .is_err();
        }
        // Sepolia
        11155111 => {
            err |= ANGSTROM_ADDRESS
                .set(address!("0x3B9172ef12bd245A07DA0d43dE29e09036626AFC"))
                .is_err();
            err |= POSITION_MANAGER_ADDRESS
                .set(address!("0x429ba70129df741B2Ca2a85BC3A2a3328e5c09b4"))
                .is_err();
            err |= CONTROLLER_V1_ADDRESS
                .set(address!("0x977c67e6CEe5b5De090006E87ADaFc99Ebed2a7A"))
                .is_err();
            err |= POOL_MANAGER_ADDRESS
                .set(address!("0xE03A1074c86CFeDd5C142C4F04F1a1536e203543"))
                .is_err();
            err |= CHAIN_ID.set(11155111).is_err();
            err |= ANGSTROM_DEPLOYED_BLOCK.set(8578780).is_err();
            err |= GAS_TOKEN_ADDRESS
                .set(address!("0xfff9976782d46cc05630d1f6ebab18b2324d6b14"))
                .is_err();
            err |= ANGSTROM_DOMAIN
                .set(alloy::sol_types::eip712_domain!(
                    name: "Angstrom",
                    version: "v1",
                    chain_id: 11155111,
                    verifying_contract: address!("0x3B9172ef12bd245A07DA0d43dE29e09036626AFC"),
                ))
                .is_err();
        }
        // Unichain
        130 => {
            err |= ANGSTROM_ADDRESS.set(Address::ZERO).is_err();
            err |= POSITION_MANAGER_ADDRESS.set(Address::ZERO).is_err();
            err |= CONTROLLER_V1_ADDRESS.set(Address::ZERO).is_err();
            err |= POOL_MANAGER_ADDRESS.set(Address::ZERO).is_err();
            err |= CHAIN_ID.set(130).is_err();
            err |= ANGSTROM_DEPLOYED_BLOCK.set(0).is_err();
            err |= GAS_TOKEN_ADDRESS.set(Address::ZERO).is_err();
            err |= ANGSTROM_DOMAIN
                .set(alloy::sol_types::eip712_domain!(
                    name: "Angstrom",
                    version: "v1",
                    chain_id: 130,
                    verifying_contract: Address::ZERO,
                ))
                .is_err();
        }
        // Unichain Sepolia
        1301 => {
            err |= ANGSTROM_ADDRESS.set(Address::ZERO).is_err();
            err |= POSITION_MANAGER_ADDRESS.set(Address::ZERO).is_err();
            err |= CONTROLLER_V1_ADDRESS.set(Address::ZERO).is_err();
            err |= POOL_MANAGER_ADDRESS.set(Address::ZERO).is_err();
            err |= CHAIN_ID.set(1301).is_err();
            err |= ANGSTROM_DEPLOYED_BLOCK.set(0).is_err();
            err |= GAS_TOKEN_ADDRESS.set(Address::ZERO).is_err();
            err |= ANGSTROM_DOMAIN
                .set(alloy::sol_types::eip712_domain!(
                    name: "Angstrom",
                    version: "v1",
                    chain_id: 1301,
                    verifying_contract: Address::ZERO,
                ))
                .is_err();
        }
        // Base
        8453 => {
            err |= ANGSTROM_ADDRESS.set(Address::ZERO).is_err();
            err |= POSITION_MANAGER_ADDRESS.set(Address::ZERO).is_err();
            err |= CONTROLLER_V1_ADDRESS.set(Address::ZERO).is_err();
            err |= POOL_MANAGER_ADDRESS.set(Address::ZERO).is_err();
            err |= CHAIN_ID.set(8453).is_err();
            err |= ANGSTROM_DEPLOYED_BLOCK.set(0).is_err();
            err |= GAS_TOKEN_ADDRESS.set(Address::ZERO).is_err();
            err |= ANGSTROM_DOMAIN
                .set(alloy::sol_types::eip712_domain!(
                    name: "Angstrom",
                    version: "v1",
                    chain_id: 8453,
                    verifying_contract: Address::ZERO,
                ))
                .is_err();
        }
        // Base Sepolia
        84532 => {
            err |= ANGSTROM_ADDRESS.set(Address::ZERO).is_err();
            err |= POSITION_MANAGER_ADDRESS.set(Address::ZERO).is_err();
            err |= CONTROLLER_V1_ADDRESS.set(Address::ZERO).is_err();
            err |= POOL_MANAGER_ADDRESS.set(Address::ZERO).is_err();
            err |= CHAIN_ID.set(84532).is_err();
            err |= ANGSTROM_DEPLOYED_BLOCK.set(0).is_err();
            err |= GAS_TOKEN_ADDRESS.set(Address::ZERO).is_err();
            err |= ANGSTROM_DOMAIN
                .set(alloy::sol_types::eip712_domain!(
                    name: "Angstrom",
                    version: "v1",
                    chain_id: 84532,
                    verifying_contract: Address::ZERO,
                ))
                .is_err();
        }
        id => panic!("unsupported chain_id: {id}")
    }
    if err {
        return Err(eyre::eyre!("one or more statics failed to set"));
    }

    Ok(())
}

pub fn init_with_chain_id(chain_id: ChainId) {
    try_init_with_chain_id(chain_id).unwrap();
}

#[derive(Debug, Default, Clone)]
pub struct UniswapPoolRegistry {
    pub pools:          HashMap<PoolId, PoolKey>,
    pub conversion_map: HashMap<PoolId, PoolId>
}

impl UniswapPoolRegistry {
    pub fn get(&self, pool_id: &PoolId) -> Option<&PoolKey> {
        self.pools.get(pool_id)
    }

    pub fn pools(&self) -> HashMap<PoolId, PoolKey> {
        self.pools.clone()
    }

    pub fn private_keys(&self) -> impl Iterator<Item = PoolId> + '_ {
        self.conversion_map.values().copied()
    }

    pub fn public_keys(&self) -> impl Iterator<Item = PoolId> + '_ {
        self.conversion_map.keys().copied()
    }
}

impl From<Vec<PoolKey>> for UniswapPoolRegistry {
    fn from(pools: Vec<PoolKey>) -> Self {
        let pubmap = pools
            .iter()
            .map(|pool_key| {
                let pool_id = PoolId::from(*pool_key);
                (pool_id, *pool_key)
            })
            .collect();

        let priv_map = pools
            .into_iter()
            .map(|mut pool_key| {
                let pool_id_pub = PoolId::from(pool_key);
                pool_key.fee = U24::from(0x800000);
                let pool_id_priv = PoolId::from(pool_key);
                (pool_id_pub, pool_id_priv)
            })
            .collect();
        Self { pools: pubmap, conversion_map: priv_map }
    }
}
