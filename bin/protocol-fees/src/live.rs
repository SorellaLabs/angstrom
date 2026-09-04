//! Reviewed deployment hashes include every immutable byte. The review must establish their
//! collector/PoolManager wiring from compiler artifacts; this module rechecks that exact code.
use crate::types::{angstrom_address, controller_v1_address};
use alloy_eips::{BlockId, BlockNumHash};
use alloy_primitives::{Address, B256, Bytes, U256, address, keccak256};
use alloy_provider::Provider;
use alloy_rpc_types::{Filter, TransactionRequest};
use alloy_sol_types::{SolCall, SolValue};
use angstrom_types_primitives::contract_bindings::controller_v_1::ControllerV1;
use eyre::{Result, ensure, eyre};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Review these values against retained compiler/deployment/governance evidence before running.
/// `assets` is the exact reviewed currency set with reviewed exact-transfer token behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub collector: Address,
    pub caller: Address,
    pub recipient: Address,
    pub assets: Vec<Address>,
    pub exact_transfer_tokens: Vec<Address>,
    pub include_native: bool,
    pub pool_manager_creation_tx: B256,
    pub angstrom_creation_tx: B256,
    /// Exact deployed code hashes, including immutables; include the recipient (empty code for EOA).
    pub runtime_hashes: BTreeMap<Address, B256>,
    pub timelock_deployment: BlockNumHash,
    pub timelock_delay: U256,
    pub timelock_roles: BTreeMap<B256, Vec<Address>>,
    pub safe_owners: Vec<Address>,
    pub safe_threshold: U256,
    pub safe_modules: Vec<Address>,
    pub safe_implementation: Address,
    pub safe_guard: Address,
    pub safe_fallback: Address,
}

fn pool_manager() -> Address {
    *angstrom_types_primitives::POOL_MANAGER_ADDRESS
        .get()
        .unwrap()
}
fn roles() -> [B256; 4] {
    [
        B256::ZERO,
        keccak256("PROPOSER_ROLE"),
        keccak256("EXECUTOR_ROLE"),
        keccak256("CANCELLER_ROLE"),
    ]
}
fn unique(values: &[Address]) -> bool {
    values.iter().collect::<BTreeSet<_>>().len() == values.len()
}
fn word(address: Address) -> U256 {
    U256::from_be_slice(address.as_slice())
}

impl Review {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.collector.is_zero() && !self.caller.is_zero(), "missing collector/caller");
        ensure!(
            ![
                Address::ZERO,
                angstrom_address(),
                controller_v1_address(),
                pool_manager(),
                self.collector
            ]
            .contains(&self.recipient),
            "forbidden recipient"
        );
        ensure!(
            !self.assets.is_empty() && unique(&self.assets),
            "assets must be nonempty and unique"
        );
        ensure!(!self.assets.contains(&self.recipient), "recipient is a collected token");
        ensure!(
            self.include_native || !self.assets.contains(&Address::ZERO),
            "native currency not approved"
        );
        ensure!(
            unique(&self.exact_transfer_tokens)
                && !self.exact_transfer_tokens.contains(&Address::ZERO)
                && self
                    .assets
                    .iter()
                    .filter(|a| !a.is_zero())
                    .all(|a| self.exact_transfer_tokens.contains(a)),
            "review exact-transfer, non-rebasing behavior for every token"
        );
        ensure!(
            !self.exact_transfer_tokens.contains(&self.recipient),
            "token contracts are forbidden recipients"
        );
        ensure!(
            !self.pool_manager_creation_tx.is_zero() && !self.angstrom_creation_tx.is_zero(),
            "missing CREATE evidence"
        );
        ensure!(
            self.timelock_deployment.number > 0 && !self.timelock_deployment.hash.is_zero(),
            "missing Timelock deployment"
        );
        ensure!(
            self.timelock_roles.keys().copied().collect::<BTreeSet<_>>()
                == roles().into_iter().collect(),
            "review all four Timelock roles"
        );
        for (role, accounts) in &self.timelock_roles {
            ensure!(!accounts.is_empty() && unique(accounts), "missing/duplicate role members");
            ensure!(
                *role == keccak256("EXECUTOR_ROLE") || !accounts.contains(&Address::ZERO),
                "zero privileged role member"
            );
        }
        ensure!(
            !self.safe_owners.is_empty() && unique(&self.safe_owners),
            "missing/duplicate Safe owners"
        );
        ensure!(
            !self.safe_owners.contains(&Address::ZERO)
                && !self
                    .safe_owners
                    .contains(&address!("0000000000000000000000000000000000000001")),
            "invalid Safe owner"
        );
        ensure!(
            self.safe_threshold > U256::ZERO
                && self.safe_threshold <= U256::from(self.safe_owners.len()),
            "invalid Safe threshold"
        );
        ensure!(
            unique(&self.safe_modules) && !self.safe_modules.contains(&Address::ZERO),
            "invalid Safe modules"
        );
        ensure!(
            !self.safe_implementation.is_zero() && self.timelock_delay > U256::ZERO,
            "missing governance configuration"
        );
        ensure!(
            self.runtime_hashes.values().all(|hash| !hash.is_zero()),
            "missing reviewed runtime hash"
        );
        Ok(())
    }
}

async fn canonical<P: Provider>(p: &P, block: BlockNumHash) -> Result<()> {
    let current = p
        .get_block_by_number(block.number.into())
        .await?
        .ok_or_else(|| eyre!("missing block"))?;
    ensure!(
        current.header.hash == block.hash && current.header.number == block.number,
        "block reorganized"
    );
    Ok(())
}

async fn expected_call<P: Provider>(
    p: &P,
    block: BlockId,
    to: Address,
    signature: &str,
    arguments: Vec<u8>,
    expected: Vec<u8>,
) -> Result<()> {
    let mut data = keccak256(signature).as_slice()[..4].to_vec();
    data.extend(arguments);
    let tx = TransactionRequest::default()
        .to(to)
        .input(Bytes::from(data).into());
    ensure!(p.call(tx).block(block).await?.as_ref() == expected.as_slice(), "{signature} changed");
    Ok(())
}

/// Check every mandatory endpoint, immutable-bearing code hash, authority and governance setting.
pub async fn check<P: Provider>(
    p: &P,
    block: BlockNumHash,
    review: &Review,
    previously_checked: Option<BlockNumHash>,
) -> Result<()> {
    review.validate()?;
    ensure!(p.get_chain_id().await? == 1, "expected Ethereum mainnet");
    canonical(p, block).await?;
    let pinned = BlockId::hash_canonical(block.hash);
    let controller = ControllerV1::new(controller_v1_address(), p);
    let (angstrom, owner, fast, pending) = tokio::try_join!(
        async { controller.ANGSTROM().block(pinned).call().await },
        async { controller.owner().block(pinned).call().await },
        async { controller.fastOwner().block(pinned).call().await },
        async { controller.setController().block(pinned).call().await }
    )?;
    ensure!(
        angstrom == angstrom_address() && pending.is_zero(),
        "controller wiring or pending controller changed"
    );
    ensure!(
        !owner.is_zero() && !fast.is_zero() && (review.caller == owner || review.caller == fast),
        "caller is not owner/fastOwner"
    );
    let mut required = BTreeSet::from([
        angstrom,
        controller_v1_address(),
        pool_manager(),
        review.collector,
        owner,
        fast,
        review.safe_implementation,
        review.recipient,
    ]);
    required.extend(review.exact_transfer_tokens.iter().copied());
    required.extend(review.safe_modules.iter().copied());
    required.extend(
        [review.safe_guard, review.safe_fallback]
            .into_iter()
            .filter(|a| !a.is_zero()),
    );
    ensure!(
        required
            .iter()
            .all(|a| review.runtime_hashes.contains_key(a)),
        "missing runtime/immutable review"
    );
    for (address, hash) in &review.runtime_hashes {
        let code = p.get_code_at(*address).block_id(pinned).await?;
        ensure!(
            !required.contains(address) || *address == review.recipient || !code.is_empty(),
            "missing contract code"
        );
        ensure!(
            *address != review.recipient || code.is_empty(),
            "use a dedicated recipient account without token/contract code"
        );
        ensure!(keccak256(code) == *hash, "runtime changed at {address}");
    }
    ensure!(
        p.get_storage_at(angstrom, U256::ZERO)
            .block_id(pinned)
            .await?
            == word(controller_v1_address()),
        "Angstrom controller changed"
    );
    if let Some(previous) = previously_checked {
        ensure!(previous.number <= block.number, "preflight cannot move backwards");
        canonical(p, previous).await?;
    }
    timelock(p, block, owner, review, previously_checked).await?;
    let sentinel = address!("0000000000000000000000000000000000000001");
    ensure!(!review.safe_modules.contains(&sentinel), "sentinel is not a Safe module");
    expected_call(p, pinned, fast, "getOwners()", vec![], review.safe_owners.abi_encode()).await?;
    expected_call(p, pinned, fast, "getThreshold()", vec![], review.safe_threshold.abi_encode())
        .await?;
    expected_call(p, pinned, fast, "masterCopy()", vec![], review.safe_implementation.abi_encode())
        .await?;
    expected_call(
        p,
        pinned,
        fast,
        "getModulesPaginated(address,uint256)",
        (sentinel, U256::from(review.safe_modules.len() + 1)).abi_encode_params(),
        (review.safe_modules.clone(), sentinel).abi_encode_params(),
    )
    .await?;
    for (slot, expected) in [
        (B256::ZERO, review.safe_implementation),
        (keccak256("guard_manager.guard.address"), review.safe_guard),
        (keccak256("fallback_manager.handler.address"), review.safe_fallback),
    ] {
        ensure!(
            p.get_storage_at(fast, U256::from_be_slice(slot.as_slice()))
                .block_id(pinned)
                .await?
                == word(expected),
            "Safe storage configuration changed"
        );
    }
    canonical(p, block).await
}

async fn timelock<P: Provider>(
    p: &P,
    block: BlockNumHash,
    owner: Address,
    review: &Review,
    previously_checked: Option<BlockNumHash>,
) -> Result<()> {
    let deployment = review.timelock_deployment;
    ensure!(deployment.number <= block.number, "Timelock deployment is after accounting boundary");
    canonical(p, deployment).await?;
    let previous = p
        .get_block_by_number((deployment.number - 1).into())
        .await?
        .ok_or_else(|| eyre!("missing deployment prestate"))?;
    ensure!(previous.header.number == deployment.number - 1, "deployment prestate number mismatch");
    ensure!(
        p.get_code_at(owner)
            .block_id(BlockId::hash_canonical(previous.header.hash))
            .await?
            .is_empty(),
        "Timelock opening code is not empty"
    );
    ensure!(
        !p.get_code_at(owner)
            .block_id(BlockId::hash_canonical(deployment.hash))
            .await?
            .is_empty(),
        "Timelock creation missing"
    );
    let pinned = BlockId::hash_canonical(block.hash);
    expected_call(p, pinned, owner, "getMinDelay()", vec![], review.timelock_delay.abi_encode())
        .await?;
    let grant = keccak256("RoleGranted(bytes32,address,address)");
    let revoke = keccak256("RoleRevoked(bytes32,address,address)");
    let mut members: BTreeMap<B256, BTreeSet<Address>> =
        roles().into_iter().map(|r| (r, BTreeSet::new())).collect();
    let mut seen = BTreeSet::new();
    let mut known = BTreeSet::from([Address::ZERO]);
    let mut from = deployment.number;
    if let Some(previous) = previously_checked {
        // The caller completed the full replay in this run before requesting this short extension.
        members = review
            .timelock_roles
            .iter()
            .map(|(role, accounts)| (*role, accounts.iter().copied().collect()))
            .collect();
        known.extend(review.timelock_roles.values().flatten().copied());
        from = previous
            .number
            .checked_add(1)
            .ok_or_else(|| eyre!("block overflow"))?;
    }
    while from <= block.number {
        let to = from.saturating_add(4_999).min(block.number);
        let filter = Filter::new()
            .address(owner)
            .event_signature(vec![grant, revoke])
            .from_block(from)
            .to_block(to);
        let mut logs = p.get_logs(&filter).await?;
        for log in &logs {
            ensure!(
                !log.removed
                    && log.address() == owner
                    && log.block_number.is_some_and(|n| (from..=to).contains(&n))
                    && log.block_hash.is_some()
                    && log.transaction_hash.is_some()
                    && log.transaction_index.is_some()
                    && log.log_index.is_some(),
                "incomplete role log"
            );
        }
        logs.sort_by_key(|l| (l.block_number, l.transaction_index, l.log_index));
        for log in logs {
            ensure!(
                seen.insert((log.block_hash.unwrap(), log.log_index.unwrap())),
                "duplicate role log"
            );
            canonical(
                p,
                BlockNumHash { number: log.block_number.unwrap(), hash: log.block_hash.unwrap() },
            )
            .await?;
            let topics = log.topics();
            ensure!(topics.len() == 4 && log.data().data.is_empty(), "malformed role event");
            let account = Address::abi_decode_validate(topics[2].as_slice())?;
            Address::abi_decode_validate(topics[3].as_slice())?;
            let set = members
                .get_mut(&topics[1])
                .ok_or_else(|| eyre!("unreviewed Timelock role"))?;
            ensure!(
                if topics[0] == grant {
                    set.insert(account)
                } else {
                    topics[0] == revoke && set.remove(&account)
                },
                "inconsistent role history"
            );
            known.insert(account);
        }
        if to == block.number {
            break;
        }
        from = to + 1;
    }
    for role in roles() {
        let expected: BTreeSet<_> = review.timelock_roles[&role].iter().copied().collect();
        ensure!(members[&role] == expected, "Timelock role membership changed");
        expected_call(
            p,
            pinned,
            owner,
            "getRoleAdmin(bytes32)",
            role.abi_encode(),
            B256::ZERO.abi_encode(),
        )
        .await?;
        for account in &known {
            expected_call(
                p,
                pinned,
                owner,
                "hasRole(bytes32,address)",
                (role, *account).abi_encode_params(),
                expected.contains(account).abi_encode(),
            )
            .await?;
        }
    }
    Ok(())
}

/// Simulate exactly the reviewed bytes at the freshest numbered block. No transaction is sent.
pub async fn simulate<P: Provider>(
    p: &P,
    block: BlockNumHash,
    review: &Review,
    calldata: &Bytes,
) -> Result<()> {
    review.validate()?;
    let call = ControllerV1::collect_unlock_swap_feesCall::abi_decode_validate(calldata)?;
    ensure!(
        call.abi_encode() == calldata.as_ref() && call.to == review.recipient,
        "calldata/recipient mismatch"
    );
    let packed: Vec<_> = review
        .assets
        .iter()
        .flat_map(|asset| asset.as_slice().iter().copied())
        .collect();
    ensure!(
        call.packed_assets.as_ref() == packed.as_slice(),
        "calldata differs from reviewed asset set"
    );
    ensure!(
        p.get_block_number().await? == block.number,
        "extend replay to the freshest block before simulation"
    );
    canonical(p, block).await?;
    let pinned = BlockId::hash_canonical(block.hash);
    let controller = ControllerV1::new(controller_v1_address(), p);
    let (owner, fast) =
        tokio::try_join!(async { controller.owner().block(pinned).call().await }, async {
            controller.fastOwner().block(pinned).call().await
        })?;
    ensure!(review.caller == owner || review.caller == fast, "unauthorized simulation caller");
    let tx = TransactionRequest::default()
        .from(review.caller)
        .to(controller_v1_address())
        .value(U256::ZERO)
        .input(calldata.clone().into());
    ensure!(p.call(tx).block(pinned).await?.is_empty(), "unexpected collection simulation return");
    canonical(p, block).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_review_is_not_accepted() {
        assert!(serde_json::from_str::<Review>("{}").is_err());
    }
    #[test]
    fn governance_array_encoding_is_canonical() {
        let owners = vec![Address::repeat_byte(1), Address::repeat_byte(2)];
        let encoded = owners.abi_encode();
        assert_eq!(Vec::<Address>::abi_decode_validate(&encoded).unwrap(), owners);
        assert!(unique(&owners));
        assert!(!unique(&[owners[0], owners[0]]));
    }
}
