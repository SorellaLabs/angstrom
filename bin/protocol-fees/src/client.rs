use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::Arc,
};

use alloy_eips::{BlockId, BlockNumHash};
use alloy_primitives::{Address, B256, Bytes, U256, keccak256};
use alloy_provider::{Provider, ProviderBuilder, WsConnect, ext::DebugApi};
use alloy_rpc_types::{Filter, Log, TransactionReceipt, TransactionRequest};
use alloy_rpc_types_trace::geth::{CallConfig, CallFrame, GethDebugTracingOptions};
use alloy_sol_types::{SolCall, SolEvent, SolValue};
use angstrom_types_primitives::{
    ERC20, POOL_MANAGER_ADDRESS,
    contract_bindings::{controller_v_1::ControllerV1, pool_manager::PoolManager},
};
use eyre::{Context, Result, ensure, eyre};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    live::{self, Review},
    types::angstrom_address,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    review: Review,
    second_ws_url: String,
    evidence_directory: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct Collection {
    pub block: BlockNumHash,
    pub calldata: Bytes,
    pub fees: Vec<TokenFees>,
}

#[derive(Debug, Serialize)]
pub struct TokenFees {
    pub asset: Address,
    pub symbol: String,
    pub amount: String,
    pub raw_amount: U256,
}

#[derive(Debug, Default, Serialize)]
struct Claims {
    protocol: U256,
    incidental: U256,
}

impl Claims {
    fn total(&self) -> Result<U256> {
        checked_add(self.protocol, self.incidental)
    }

    fn credit(&mut self, amount: U256, protocol: bool) -> Result<()> {
        let balance = if protocol { &mut self.protocol } else { &mut self.incidental };
        *balance = checked_add(*balance, amount)?;
        Ok(())
    }

    /// Returns true only for a whole-balance collector burn, which still requires trace/payment proof.
    fn apply(
        &mut self,
        event: &PoolManager::Transfer,
        collector: Address,
        after_create: bool,
    ) -> Result<bool> {
        if event.from == collector {
            if after_create {
                ensure!(
                    event.to == Address::ZERO && event.caller == collector,
                    "unexpected outgoing collector claim"
                );
                ensure!(
                    event.amount == self.total()? && event.amount <= U256::from(i128::MAX as u128),
                    "burn is not the whole permitted balance"
                );
                *self = Self::default();
                return Ok(true);
            }
            self.incidental = self
                .incidental
                .checked_sub(event.amount)
                .ok_or_else(|| eyre!("uncovered precreation outflow"))?;
            if event.to == collector {
                self.credit(event.amount, false)?;
            }
        } else {
            ensure!(event.to == collector, "log outside collector universe");
            self.credit(
                event.amount,
                after_create
                    && event.from == Address::ZERO
                    && event.caller == angstrom_address()
                    && event.id < (U256::from(1) << 160),
            )?;
        }
        Ok(false)
    }
}

pub struct ProtocolFeeFetcher<P: Provider> {
    provider: Arc<P>,
    max_block: BlockNumHash,
}

impl<P: Provider> ProtocolFeeFetcher<P> {
    pub async fn new(provider: P) -> Result<Self> {
        let max_block = pin(&provider, provider.get_block_number().await?).await?;
        Ok(Self { provider: Arc::new(provider), max_block })
    }

    pub async fn collectable_fees(&self) -> Result<Collection> {
        let path = std::env::var_os("PROTOCOL_FEES_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| "bin/protocol-fees/config.json".into());
        let input: Inputs =
            serde_json::from_slice(&fs::read(&path).wrap_err_with(|| {
                format!("read reviewed collection inputs at {}", path.display())
            })?)?;
        let other = ProviderBuilder::new()
            .connect_ws(WsConnect::new(&input.second_ws_url))
            .await?;
        fs::create_dir(&input.evidence_directory).wrap_err("evidence directory must be new")?;
        save(&input.evidence_directory, "review.json", &input.review)?;
        let result = self.calculate(&other, &input).await;
        if let Err(error) = &result {
            save(&input.evidence_directory, "failure.json", &json!({"error":error.to_string()}))?;
        }
        result
    }

    async fn calculate<Q: Provider>(&self, other: &Q, input: &Inputs) -> Result<Collection> {
        let provider = self.provider.as_ref();
        let review = &input.review;
        review.validate()?;
        let dir = &input.evidence_directory;
        ensure!(
            provider.get_chain_id().await? == 1 && other.get_chain_id().await? == 1,
            "expected mainnet providers"
        );
        let pm = *POOL_MANAGER_ADDRESS.get().unwrap();
        let mut traces = BTreeMap::new();
        for tx in [review.pool_manager_creation_tx, review.angstrom_creation_tx] {
            let evidence = transaction(provider, tx).await?;
            let independent = transaction(other, tx).await?;
            ensure!(evidence == independent, "deployment trace/receipt disagreement");
            save(dir, &format!("{tx}.json"), &evidence)?;
            traces.insert(tx, evidence);
        }
        let pm_create = creation(&traces[&review.pool_manager_creation_tx], pm)?;
        let collector_create = creation(&traces[&review.angstrom_creation_tx], review.collector)?;
        let angstrom_create = creation(&traces[&review.angstrom_creation_tx], angstrom_address())?;
        ensure!(pm_create.0 > 0, "invalid PoolManager deployment block");
        ensure!(
            pm_create < angstrom_create && angstrom_create < collector_create,
            "incorrect deployment order"
        );
        ensure!(
            collector_create.0 <= self.max_block.number,
            "deployment after accounting boundary"
        );
        ensure!(
            provider
                .get_code_at(pm)
                .block_id((pm_create.0 - 1).into())
                .await?
                .is_empty(),
            "PoolManager deployment is not its first code"
        );
        // The reviewed deployed runtime hashes also bind every immutable, including collector ownership.
        for (tx, address) in [
            (review.pool_manager_creation_tx, pm),
            (review.angstrom_creation_tx, angstrom_address()),
            (review.angstrom_creation_tx, review.collector),
        ] {
            let (calls, _) = ordered(&traces[&tx].1)?;
            let deployed = calls
                .iter()
                .find(|(_, call)| {
                    call.to == Some(address) && matches!(call.typ.as_str(), "CREATE" | "CREATE2")
                })
                .and_then(|(_, call)| call.output.as_ref())
                .ok_or_else(|| eyre!("missing CREATE runtime"))?;
            ensure!(
                review.runtime_hashes.get(&address) == Some(&keccak256(deployed)),
                "CREATE runtime differs from review"
            );
        }
        live::check(provider, self.max_block, review, None).await?;
        let mut previously_checked = self.max_block;
        let mut logs = BTreeMap::new();
        let mut end = self.max_block;
        self.scan(other, pm_create.0, end, review.collector, dir, &mut logs)
            .await?;
        let mut start = end.number;
        for attempt in 0..3 {
            ensure!(
                pin(provider, end.number).await? == end && pin(other, end.number).await? == end,
                "previously scanned boundary reorganized; restart the replay"
            );
            end = pin(provider, provider.get_block_number().await?).await?;
            if end.number > start {
                self.scan(other, start, end, review.collector, dir, &mut logs)
                    .await?;
                start = end.number;
            }
            let mut balances = BTreeMap::<U256, Claims>::new();
            for log in logs.values() {
                let (block, tx_index, log_index) = log_position(log)?;
                let event = log
                    .log_decode_validate::<PoolManager::Transfer>()?
                    .inner
                    .data;
                let tx = log.transaction_hash.unwrap();
                if event.from == review.collector && !traces.contains_key(&tx) {
                    let evidence = transaction(provider, tx).await?;
                    ensure!(
                        evidence == transaction(other, tx).await?,
                        "outgoing trace/receipt disagreement"
                    );
                    save(dir, &format!("{tx}.json"), &evidence)?;
                    traces.insert(tx, evidence);
                }
                let position = if let Some(evidence) = traces.get(&tx) {
                    let (_, ordered_logs) = ordered(&evidence.1)?;
                    let receipt_index = evidence
                        .0
                        .logs()
                        .iter()
                        .position(|receipt_log| receipt_log == log)
                        .ok_or_else(|| eyre!("scanned log missing from receipt"))?;
                    (block, tx_index, ordered_logs[receipt_index].0)
                } else {
                    (block, tx_index, log_index)
                };
                let state = balances.entry(event.id).or_default();
                ensure!(
                    position > pm_create && position != collector_create,
                    "claim outside exact CREATE boundaries"
                );
                let before = (state.protocol, state.incidental);
                if state.apply(&event, review.collector, position > collector_create)? {
                    verify_burn(&traces[&tx].1, log, review)?;
                    save(
                        dir,
                        &format!("burn-{block}-{log_index}-{attempt}.json"),
                        &json!({"id":event.id,
                        "protocol_revenue":before.0,"incidental_to_quarantine":before.1,"whole_balance":event.amount}),
                    )?;
                }
            }
            let pool = PoolManager::new(pm, provider);
            let independent_pool = PoolManager::new(pm, other);
            for (id, state) in &balances {
                let block = BlockId::hash_canonical(end.hash);
                let (a, b) = tokio::try_join!(
                    async {
                        pool.balanceOf(review.collector, *id)
                            .block(block)
                            .call()
                            .await
                    },
                    async {
                        independent_pool
                            .balanceOf(review.collector, *id)
                            .block(block)
                            .call()
                            .await
                    }
                )?;
                ensure!(a == b && a == state.total()?, "claim conservation mismatch for {id}");
            }
            save(
                dir,
                &format!("claims-{attempt}.json"),
                &json!({"block":end,"balances":balances}),
            )?;
            ensure!(
                !balances.contains_key(&U256::from_be_slice(review.recipient.as_slice())),
                "currency token is a forbidden recipient"
            );
            live::check(provider, end, review, Some(previously_checked)).await?;
            previously_checked = end;
            let mut packed = Vec::new();
            let mut fees = Vec::new();
            let mut unique = BTreeSet::new();
            for asset in &review.assets {
                ensure!(unique.insert(*asset), "duplicate reviewed asset");
                let id = U256::from_be_slice(asset.as_slice());
                let state = balances
                    .get(&id)
                    .ok_or_else(|| eyre!("reviewed asset has no claim history"))?;
                ensure!(
                    state.incidental.is_zero(),
                    "incidental claims contaminate {asset}; omit it from protocol-only collection"
                );
                ensure!(
                    state.protocol > U256::ZERO && state.protocol <= U256::from(i128::MAX as u128),
                    "reviewed claim must be nonzero and <= int128 max"
                );
                let (symbol, decimals) = if asset.is_zero() {
                    ensure!(review.include_native, "native collection must be deliberate");
                    ("ETH".to_owned(), 18)
                } else {
                    ensure!(
                        review.exact_transfer_tokens.contains(asset),
                        "token behavior has not been reviewed"
                    );
                    let block = BlockId::hash_canonical(end.hash);
                    let symbol = TransactionRequest::default()
                        .to(*asset)
                        .input(Bytes::from(ERC20::symbolCall {}.abi_encode()).into());
                    let decimals = TransactionRequest::default()
                        .to(*asset)
                        .input(Bytes::from(ERC20::decimalsCall {}.abi_encode()).into());
                    let (symbol, decimals) = tokio::try_join!(
                        provider.call(symbol).block(block),
                        provider.call(decimals).block(block)
                    )?;
                    (
                        ERC20::symbolCall::abi_decode_returns_validate(&symbol)?,
                        ERC20::decimalsCall::abi_decode_returns_validate(&decimals)?,
                    )
                };
                ensure!(
                    !symbol.is_empty()
                        && symbol.len() <= 128
                        && !symbol.chars().any(char::is_control),
                    "invalid token symbol"
                );
                fees.push(TokenFees {
                    asset: *asset,
                    symbol,
                    amount: format_amount(state.protocol, decimals),
                    raw_amount: state.protocol,
                });
                packed.extend_from_slice(asset.as_slice());
            }
            ensure!(
                !packed.is_empty() && packed.len() % 20 == 0,
                "no reviewed currencies to collect"
            );
            let calldata: Bytes = ControllerV1::collect_unlock_swap_feesCall {
                to: review.recipient,
                packed_assets: packed.into(),
            }
            .abi_encode()
            .into();
            if provider.get_block_number().await? != end.number {
                continue;
            }
            live::simulate(provider, end, review, &calldata).await?;
            save(
                dir,
                "simulation.json",
                &json!({"block":end, "from":review.caller,
                "to":crate::types::controller_v1_address(), "value":"0x0", "data":calldata, "result":"0x"}),
            )?;
            let mut runtimes = BTreeMap::new();
            for address in review.runtime_hashes.keys() {
                runtimes.insert(
                    *address,
                    provider
                        .get_code_at(*address)
                        .block_id(BlockId::hash_canonical(end.hash))
                        .await?,
                );
            }
            save(dir, "runtimes.json", &runtimes)?;
            ensure!(
                pin(provider, end.number).await? == end && pin(other, end.number).await? == end,
                "accounting boundary reorganized"
            );
            let result = Collection { block: end, calldata, fees };
            save(dir, "collection.json", &result)?;
            return Ok(result);
        }
        eyre::bail!("head changed during preflight; rerun with a fresh evidence directory")
    }

    async fn scan<Q: Provider>(
        &self,
        other: &Q,
        start: u64,
        end: BlockNumHash,
        collector: Address,
        dir: &Path,
        logs: &mut BTreeMap<(u64, u64, u64), Log>,
    ) -> Result<()> {
        let pm = *POOL_MANAGER_ADDRESS.get().unwrap();
        let base = Filter::new()
            .address(pm)
            .event_signature(PoolManager::Transfer::SIGNATURE_HASH);
        let topic = B256::left_padding_from(collector.as_slice());
        let mut from = start;
        loop {
            let to = (from + 1999).min(end.number);
            for (direction, filter) in [base.clone().topic1(topic), base.clone().topic2(topic)]
                .into_iter()
                .enumerate()
            {
                let filter = filter.from_block(from).to_block(to);
                let (mut a, mut b) =
                    tokio::try_join!(self.provider.get_logs(&filter), other.get_logs(&filter))?;
                for log in a.iter().chain(&b) {
                    let position = log_position(log)?;
                    ensure!(
                        (from..=to).contains(&position.0) && log.address() == pm,
                        "provider returned a log outside range"
                    );
                }
                a.sort_by_key(|log| (log.block_number, log.transaction_index, log.log_index));
                b.sort_by_key(|log| (log.block_number, log.transaction_index, log.log_index));
                ensure!(a == b, "independent claim scans disagree");
                save(
                    dir,
                    &format!("logs-{from}-{to}-{direction}.json"),
                    &json!({"filter":filter,"primary":a,"secondary":b}),
                )?;
                for log in a {
                    if let Some(previous) = logs.insert(log_position(&log)?, log.clone()) {
                        ensure!(previous == log, "overlapping claim ranges disagree");
                    }
                }
            }
            if to == end.number {
                break;
            }
            from = to;
        }
        let mut hashes = BTreeMap::new();
        for log in logs.values() {
            if let Some(hash) = hashes.insert(log.block_number.unwrap(), log.block_hash.unwrap()) {
                ensure!(hash == log.block_hash.unwrap(), "claim logs mix forks");
            }
        }
        ensure!(
            pin(self.provider.as_ref(), end.number).await? == end
                && pin(other, end.number).await? == end,
            "scan boundary reorganized"
        );
        Ok(())
    }
}

async fn pin<P: Provider>(provider: &P, number: u64) -> Result<BlockNumHash> {
    let block = provider
        .get_block(number.into())
        .await?
        .ok_or_else(|| eyre!("missing numbered block"))?;
    ensure!(
        block.header.number == number && block.header.hash != B256::ZERO,
        "invalid block boundary"
    );
    Ok(BlockNumHash::new(number, block.header.hash))
}

fn save(dir: &Path, name: &str, value: &impl Serialize) -> Result<()> {
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(dir.join(name))?;
    serde_json::to_writer_pretty(&file, value)?;
    file.sync_all()?;
    Ok(())
}

fn log_position(log: &Log) -> Result<(u64, u64, u64)> {
    ensure!(
        !log.removed
            && log.block_hash.is_some_and(|x| !x.is_zero())
            && log.transaction_hash.is_some_and(|x| !x.is_zero()),
        "unconfirmed/removed log"
    );
    Ok((
        log.block_number
            .ok_or_else(|| eyre!("missing block number"))?,
        log.transaction_index
            .ok_or_else(|| eyre!("missing tx index"))?,
        log.log_index.ok_or_else(|| eyre!("missing log index"))?,
    ))
}

async fn transaction<P: Provider>(
    provider: &P,
    tx: B256,
) -> Result<(TransactionReceipt, CallFrame)> {
    let (receipt, trace) = tokio::try_join!(
        provider.get_transaction_receipt(tx),
        provider.debug_trace_transaction_call(
            tx,
            GethDebugTracingOptions::call_tracer(CallConfig::default().with_log())
        )
    )?;
    let receipt = receipt.ok_or_else(|| eyre!("missing transaction receipt"))?;
    ensure!(
        receipt.status()
            && receipt.transaction_hash == tx
            && trace.error.is_none()
            && trace.revert_reason.is_none(),
        "failed/mismatched transaction"
    );
    let number = receipt
        .block_number
        .ok_or_else(|| eyre!("missing receipt block number"))?;
    ensure!(
        Some(pin(provider, number).await?.hash) == receipt.block_hash,
        "receipt is not canonical"
    );
    ensure!(
        trace.from == receipt.from && trace.to == receipt.to.or(receipt.contract_address),
        "trace/receipt call identity differs"
    );
    let (_, logs) = ordered(&trace)?;
    ensure!(
        logs.iter()
            .map(|(_, log)| log)
            .eq(receipt.logs().iter().map(|log| &log.inner)),
        "trace logs differ from complete receipt"
    );
    let mut previous = None;
    for log in receipt.logs() {
        let position = log_position(log)?;
        ensure!(
            log.transaction_hash == Some(tx)
                && log.block_hash == receipt.block_hash
                && log.block_number == receipt.block_number
                && log.transaction_index == receipt.transaction_index,
            "receipt log identity mismatch"
        );
        ensure!(previous.is_none_or(|p| p < position), "receipt log order mismatch");
        previous = Some(position);
    }
    Ok((receipt, trace))
}

// Use Alloy's trace tree; only assign execution order, including CREATE and same-transaction logs.
type OrderedTrace<'a> = (Vec<(u64, &'a CallFrame)>, Vec<(u64, alloy_primitives::Log)>);

fn ordered(root: &CallFrame) -> Result<OrderedTrace<'_>> {
    fn visit<'a>(
        frame: &'a CallFrame,
        next: &mut u64,
        calls: &mut Vec<(u64, &'a CallFrame)>,
        logs: &mut Vec<(u64, alloy_primitives::Log)>,
    ) -> Result<()> {
        if frame.error.is_some() || frame.revert_reason.is_some() {
            return Ok(());
        }
        calls.push((*next, frame));
        *next += 1;
        let mut emitted = 0;
        for index in 0..=frame.calls.len() {
            for log in frame
                .logs
                .iter()
                .filter(|log| log.position == Some(index as u64))
            {
                ensure!(
                    log.address.is_some() && log.topics.is_some() && log.data.is_some(),
                    "incomplete trace log"
                );
                logs.push((*next, log.clone().into_log()));
                *next += 1;
                emitted += 1;
            }
            if let Some(child) = frame.calls.get(index) {
                visit(child, next, calls, logs)?;
            }
        }
        ensure!(emitted == frame.logs.len(), "trace lacks complete log positions");
        Ok(())
    }
    let (mut calls, mut logs) = (Vec::new(), Vec::new());
    visit(root, &mut 0, &mut calls, &mut logs)?;
    Ok((calls, logs))
}

fn creation(
    evidence: &(TransactionReceipt, CallFrame),
    address: Address,
) -> Result<(u64, u64, u64)> {
    let (calls, _) = ordered(&evidence.1)?;
    let found: Vec<_> = calls
        .iter()
        .filter(|(_, c)| c.to == Some(address) && matches!(c.typ.as_str(), "CREATE" | "CREATE2"))
        .collect();
    ensure!(found.len() == 1, "missing/ambiguous deployment CREATE");
    Ok((
        evidence
            .0
            .block_number
            .ok_or_else(|| eyre!("missing deployment block"))?,
        evidence
            .0
            .transaction_index
            .ok_or_else(|| eyre!("missing deployment tx index"))?,
        found[0].0,
    ))
}

fn verify_burn(root: &CallFrame, log: &Log, review: &Review) -> Result<()> {
    let event = log
        .log_decode_validate::<PoolManager::Transfer>()?
        .inner
        .data;
    ensure!(event.id < (U256::from(1) << 160), "non-currency outgoing claim");
    let currency = Address::from_slice(&event.id.to_be_bytes::<32>()[12..]);
    let pm = *POOL_MANAGER_ADDRESS.get().unwrap();
    let (calls, _) = ordered(root)?;
    let mut matches = 0;
    for (_, collector) in calls.iter().filter(|(_, c)| {
        c.typ == "CALL" && c.from == angstrom_address() && c.to == Some(review.collector)
    }) {
        let Some(arguments) = collector.input.get(4..) else {
            continue;
        };
        if collector.input[..4] != keccak256("withdraw_to(address,bytes)")[..4] {
            continue;
        }
        let (recipient, packed) = <(Address, Bytes)>::abi_decode_params_validate(arguments)?;
        ensure!(
            !packed.is_empty() && packed.len() % 20 == 0,
            "invalid historical packed currencies"
        );
        let assets: BTreeSet<_> = packed.chunks_exact(20).map(Address::from_slice).collect();
        ensure!(assets.len() == packed.len() / 20, "repeated historical currency");
        if !assets.contains(&currency) {
            continue;
        }
        let (nested, _) = ordered(collector)?;
        let burn_data =
            PoolManager::burnCall { from: review.collector, id: event.id, amount: event.amount }
                .abi_encode();
        let take_data =
            PoolManager::takeCall { currency, to: recipient, amount: event.amount }.abi_encode();
        let burns: Vec<_> = nested
            .iter()
            .filter(|(_, c)| {
                c.typ == "CALL"
                    && c.from == review.collector
                    && c.to == Some(pm)
                    && c.input.as_ref() == burn_data
            })
            .collect();
        let takes: Vec<_> = nested
            .iter()
            .filter(|(_, c)| {
                c.typ == "CALL"
                    && c.from == review.collector
                    && c.to == Some(pm)
                    && c.input.as_ref() == take_data
            })
            .collect();
        ensure!(
            burns.len() == 1 && takes.len() == 1 && burns[0].0 < takes[0].0,
            "missing/ambiguous burn and payment calls"
        );
        let (_, burn_logs) = ordered(burns[0].1)?;
        ensure!(
            burn_logs.iter().filter(|(_, l)| l == &log.inner).count() == 1,
            "burn trace does not contain claim log"
        );
        let (payments, payment_logs) = ordered(takes[0].1)?;
        if currency.is_zero() {
            ensure!(
                payments
                    .iter()
                    .filter(|(_, c)| c.typ == "CALL"
                        && c.from == pm
                        && c.to == Some(recipient)
                        && c.value == Some(event.amount))
                    .count()
                    == 1,
                "missing native payment"
            );
        } else {
            ensure!(
                review.exact_transfer_tokens.contains(&currency),
                "historical token behavior not reviewed"
            );
            let paid = payment_logs
                .iter()
                .filter(|(_, l)| l.address == currency)
                .filter_map(|(_, l)| ERC20::Transfer::decode_log_validate(l).ok())
                .filter(|l| {
                    l.data.from == pm && l.data.to == recipient && l.data.value == event.amount
                })
                .count();
            ensure!(paid == 1, "missing/ambiguous underlying token transfer");
        }
        matches += 1;
    }
    ensure!(matches == 1, "outgoing claim lacks a verified collector withdrawal");
    Ok(())
}

fn checked_add(a: U256, b: U256) -> Result<U256> {
    a.checked_add(b).ok_or_else(|| eyre!("claim overflow"))
}

fn format_amount(amount: U256, decimals: u8) -> String {
    let mut digits = amount.to_string();
    if decimals == 0 {
        return digits;
    }
    let decimals = usize::from(decimals);
    if digits.len() <= decimals {
        digits = format!("{}{}", "0".repeat(decimals + 1 - digits.len()), digits);
    }
    digits.insert(digits.len() - decimals, '.');
    digits
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summary_uses_exact_decimal_units() {
        assert_eq!(format_amount(U256::from(28_418_032_618u64), 6), "28418.032618");
        assert_eq!(format_amount(U256::from(1), 18), "0.000000000000000001");
        assert_eq!(format_amount(U256::ZERO, 18), "0");
        assert_eq!(format_amount(U256::MAX, 0), U256::MAX.to_string());
    }
    #[test]
    fn protocol_and_incidental_value_stay_separate() {
        let mut claims = Claims::default();
        claims.credit(U256::from(10), true).unwrap();
        claims.credit(U256::from(3), false).unwrap();
        assert_eq!(claims.protocol, U256::from(10));
        assert_eq!(claims.incidental, U256::from(3));
        assert_eq!(claims.total().unwrap(), U256::from(13));
        assert!(claims.credit(U256::MAX, true).is_err());
    }
    #[test]
    fn missing_trace_order_is_a_hard_stop() {
        let trace = CallFrame { logs: vec![Default::default()], ..Default::default() };
        assert!(ordered(&trace).is_err());
    }
    fn event() -> PoolManager::Transfer {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| angstrom_types_primitives::init_with_chain_id(1));
        PoolManager::Transfer {
            caller: angstrom_address(),
            from: Address::ZERO,
            to: Address::repeat_byte(7),
            id: U256::from(2),
            amount: U256::from(10),
        }
    }
    #[test]
    fn only_postcreation_angstrom_currency_mints_are_protocol_fees() {
        let mint = event();
        let mut opening = Claims::default();
        opening.apply(&mint, mint.to, false).unwrap();
        assert_eq!(opening.protocol, U256::ZERO);
        assert_eq!(opening.incidental, U256::from(10));
        let mut state = Claims::default();
        state.apply(&mint, mint.to, true).unwrap();
        assert_eq!(state.protocol, U256::from(10));
        let mut unsolicited = mint.clone();
        unsolicited.caller = Address::repeat_byte(3);
        state.apply(&unsolicited, mint.to, true).unwrap();
        let mut non_currency = mint.clone();
        non_currency.id = U256::from(1) << 160;
        state.apply(&non_currency, mint.to, true).unwrap();
        assert_eq!(state.protocol, U256::from(10));
        assert_eq!(state.incidental, U256::from(20));
    }
    #[test]
    fn only_whole_collector_burns_can_clear_claims() {
        let mint = event();
        let mut state = Claims::default();
        state.apply(&mint, mint.to, true).unwrap();
        let mut burn = PoolManager::Transfer {
            caller: mint.to,
            from: mint.to,
            to: Address::ZERO,
            id: mint.id,
            amount: U256::from(9),
        };
        assert!(state.apply(&burn, mint.to, true).is_err());
        assert_eq!(state.protocol, U256::from(10));
        burn.amount = U256::from(10);
        burn.to = Address::repeat_byte(1);
        assert!(state.apply(&burn, mint.to, true).is_err());
        burn.to = Address::ZERO;
        assert!(state.apply(&burn, mint.to, true).unwrap());
        assert_eq!(state.total().unwrap(), U256::ZERO);
        state.protocol = U256::from(i128::MAX as u128) + U256::from(1);
        burn.amount = state.protocol;
        assert!(state.apply(&burn, mint.to, true).is_err());
    }
    #[test]
    fn reverted_ancestor_discards_successful_descendants() {
        let trace = CallFrame {
            error: Some("execution reverted".into()),
            calls: vec![CallFrame::default()],
            ..Default::default()
        };
        let (calls, logs) = ordered(&trace).unwrap();
        assert!(calls.is_empty() && logs.is_empty());
    }
}
