use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc
};

use alloy_consensus::Transaction;
use alloy_eips::{BlockId, BlockNumHash};
use alloy_primitives::{Address, Bytes, U256, utils::format_units};
use alloy_provider::{Provider, network::TransactionResponse};
use alloy_rpc_types::{Filter, Log};
use alloy_sol_types::{SolCall, SolEvent, SolValue};
use angstrom_types_primitives::{
    ERC20,
    contract_bindings::{
        angstrom::Angstrom, controller_v_1::ControllerV1, mintable_mock_erc_20::MintableMockERC20,
        pool_manager::PoolManager
    },
    contract_payloads::{Asset, angstrom::AngstromBundle}
};
use eyre::{Context, Result, ensure, eyre};
use futures::StreamExt;
use itertools::Itertools;
use pade::PadeDecode;

use crate::types::*;

pub struct ProtocolFeeFetcher<P> {
    provider:  Arc<P>,
    max_block: u64
}

impl<P: Provider> ProtocolFeeFetcher<P> {
    pub async fn new(provider: P) -> Result<Self> {
        let max_block = provider.get_block_number().await?;
        Ok(Self { provider: Arc::new(provider), max_block })
    }

    pub async fn calculate(&self) -> Result<ProtocolFeeCalculationBuilder> {
        let mut all_collections = self.all_distribute_fees().await?;
        let mut all_bundle_saves = self.all_angstrom_bundle_assets().await?;

        let saved_tokens = all_bundle_saves
            .iter()
            .flat_map(|(_, assets)| assets.iter().map(|asset| asset.addr))
            .collect::<HashSet<_>>();

        let all_tokens = self.get_all_tokens(&saved_tokens).await?;

        let block_data = (angstrom_deployed_block()..=self.max_block)
            .filter_map(|block_num| {
                let distribute = all_collections.remove(&block_num);
                let saved = all_bundle_saves.remove(&block_num);
                if distribute.is_none() && saved.is_none() {
                    None
                } else {
                    Some((block_num, distribute, saved))
                }
            })
            .map(|(block_number, distribute, saved)| ProtocolFeeBlockCalculationBuilder {
                block_number,
                saves: saved.unwrap_or_default(),
                distribute_calls: distribute.unwrap_or_default()
            })
            .collect::<Vec<_>>();

        Ok(ProtocolFeeCalculationBuilder { blocks: block_data, tokens: all_tokens })
    }

    async fn get_all_tokens(&self, tokens: &HashSet<Address>) -> eyre::Result<Vec<TokenMeta>> {
        let provider = self.provider.clone();
        futures::future::try_join_all(tokens.into_iter().map(|asset| {
            let provider = &provider;
            async move {
                let token = MintableMockERC20::new(*asset, &provider);
                let (symbol, decimals) =
                    tokio::try_join!(async { token.symbol().call().await }, async {
                        token.decimals().call().await
                    })?;

                Ok(TokenMeta { symbol, decimals, asset: *asset })
            }
        }))
        .await
    }

    async fn get_logs_over_angstrom_range<T>(
        &self,
        base_filter: Filter,
        log_kind: &str,
        transform_fn: impl Fn(Vec<Log>) -> eyre::Result<Vec<DecodedLogWithMeta<T>>> + Copy
    ) -> Result<Vec<DecodedLogWithMeta<T>>> {
        let total_blocks = self.max_block - angstrom_deployed_block() + 1;
        let block_chunks = (angstrom_deployed_block()..=self.max_block)
            .step_by(2_000)
            .map(|from| (from, (from + 1_999).min(self.max_block)))
            .collect::<Vec<_>>();

        let provider = self.provider.clone();
        let mut buffered_log_stream = futures::stream::iter(block_chunks)
            .map(|(from, to)| {
                let provider = provider.clone();
                let filter = base_filter.clone().from_block(from).to_block(to);
                async move {
                    let logs = provider.get_logs(&filter).await?;

                    let valid_logs = transform_fn(logs)?;
                    eyre::Ok(((from, to), valid_logs))
                }
            })
            .buffer_unordered(100);

        // The chunks complete out of order, so collect them all and restore execution
        // order once the whole range is in.
        let mut total_progress = 0;
        let mut all_valid_logs = Vec::new();
        while let Some(((from, to), batch_logs)) = buffered_log_stream.next().await.transpose()? {
            total_progress += to - from + 1;
            all_valid_logs.extend(batch_logs);
            tracing::info!(
                log_kind,
                blocks_searched = total_progress,
                total_blocks,
                total_progress = (total_progress as f64 / total_blocks as f64),
                "log fetch progress - completed batch"
            )
        }

        tracing::info!(log_kind, total_blocks, "log fetch progress - COMPLETE");

        // Position alone orders these, so `T` itself needs no comparison bounds.
        all_valid_logs.sort_unstable_by_key(|log| (log.block_number, log.tx_index, log.log_index));

        Ok(all_valid_logs)
    }

    async fn all_distribute_fees(
        &self
    ) -> Result<HashMap<u64, Vec<DecodedLogWithMeta<ControllerV1::distributeFeesCall>>>> {
        let owner = ControllerV1::new(controller_v1_address(), &self.provider)
            .owner()
            .block(self.max_block.into())
            .call()
            .await?;
        let filter = Filter::new()
            .address(owner)
            .event("CallExecuted(bytes32,uint256,address,uint256,bytes)");

        let logs_with_data = self
            .get_logs_over_angstrom_range(filter, "DISTRIBUTE FEES", decode_distribute_fees_logs)
            .await?;

        Ok(logs_with_data
            .into_iter()
            .map(|log| (log.block_number, log))
            .into_group_map())
    }

    async fn all_angstrom_bundle_assets(&self) -> Result<HashMap<u64, Vec<Asset>>> {
        let filter = Filter::new()
            .event_signature(PoolManager::Swap::SIGNATURE_HASH)
            .address(pool_manager_address());

        let logs_with_data = self
            .get_logs_over_angstrom_range(
                filter,
                "POOL MANAGER ANGSTROM SWAPS",
                decode_angstrom_pool_manager_logs
            )
            .await?;

        let successful_txs = logs_with_data
            .iter()
            .map(|log| log.tx_hash)
            .collect::<HashSet<_>>();
        let block_numbers = logs_with_data
            .into_iter()
            .map(|log| log.block_number)
            .collect::<HashSet<_>>();

        let total_blocks = block_numbers.len();

        let provider = self.provider.clone();
        let mut bundle_stream = futures::stream::iter(block_numbers)
            .map(|block_number| {
                let provider = provider.clone();
                let successful_txs = &successful_txs;
                async move {
                    // `into_transactions` yields nothing unless the block was
                    // requested with full bodies.
                    let block = provider
                        .get_block_by_number(block_number.into())
                        .full()
                        .await?
                        .ok_or_else(|| eyre!("missing bundle block {block_number}"))?;

                    let txs = block.transactions.into_transactions().filter(|tx| {
                        tx.to() == Some(angstrom_address())
                            && successful_txs.contains(&tx.tx_hash())
                    });

                    let mut assets = Vec::new();

                    for transaction in txs {
                        let input: &[u8] = transaction.input();
                        let Some(call) = Angstrom::executeCall::abi_decode(input).ok() else {
                            continue;
                        };

                        let tx_hash = transaction.tx_hash();
                        let is_success = provider
                            .get_transaction_receipt(tx_hash)
                            .await?
                            .ok_or_else(|| eyre::eyre!("tx does not exist: {tx_hash:?}"))?
                            .status();
                        if is_success {
                            let mut input = call.encoded.as_ref();

                            let decoded_input = AngstromBundle::pade_decode(&mut input, None)?;

                            assets.push((transaction.tx_hash(), decoded_input.assets));
                            break;
                        }
                    }

                    eyre::Ok((block_number, assets))
                }
            })
            .buffer_unordered(1000);

        let mut all_assets_with_block = HashMap::new();

        let mut total_progress = 0;
        while let Some(output) = bundle_stream.next().await {
            let (block_number, assets) = output?;
            assert!(assets.len() <= 1);

            if let Some((_, block_assets)) = assets.into_iter().next() {
                all_assets_with_block.insert(block_number, block_assets);
            }
            total_progress += 1;
            if total_progress % 1000 == 0 {
                tracing::info!(
                    blocks_searched = total_progress,
                    total_blocks,
                    total_progress = (total_progress as f64 / total_blocks as f64),
                    "bundle fetch progress - completed batch"
                )
            }
        }

        tracing::info!(total_blocks, "bundle fetch progress - COMPLETE");

        Ok(all_assets_with_block)
    }
}

#[cfg(test)]
mod tests {
    use alloy_consensus::{Signed, TxEnvelope, TxLegacy, transaction::Recovered};
    use alloy_primitives::{B256, Signature, U64, keccak256};
    use alloy_provider::ProviderBuilder;
    use alloy_transport::mock::Asserter;
    use angstrom_types_primitives::{
        contract_bindings::angstrom::Angstrom::executeCall,
        contract_payloads::{Asset, angstrom::AngstromBundle}
    };
    use pade::PadeEncode;

    use super::*;

    async fn client() -> (Asserter, ProtocolFeeFetcher<impl Provider>) {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| angstrom_types_primitives::init_with_chain_id(1));
        let rpc = Asserter::new();
        rpc.push_success(&U64::from(angstrom_deployed_block() + 10));
        let provider = ProviderBuilder::new().connect_mocked_client(rpc.clone());
        (rpc, ProtocolFeeFetcher::new(provider).await.unwrap())
    }

    fn log(index: u64, data: Bytes) -> Log {
        Log {
            inner: alloy_primitives::Log::new_unchecked(angstrom_address(), vec![], data),
            block_number: Some(angstrom_deployed_block() + 10),
            log_index: Some(index),
            transaction_hash: Some(B256::repeat_byte(index as u8)),
            ..Default::default()
        }
    }

    fn distribution(index: u64, target: Address, total: Option<u64>) -> Log {
        let mut call = ControllerV1::distributeFeesCall::default();
        if let Some(total) = total {
            call.assets.resize_with(1, Default::default);
            call.assets[0].addr = Address::repeat_byte(1);
            call.assets[0].total = U256::from(total);
            call.assets[0].dists.resize_with(1, Default::default);
            call.assets[0].dists[0].to = Address::repeat_byte(2);
            call.assets[0].dists[0].amount = U256::from(total);
        }
        log(
            index,
            (target, U256::ZERO, Bytes::from(call.abi_encode()))
                .abi_encode_params()
                .into()
        )
    }

    #[tokio::test]
    async fn calculation_keeps_the_constructor_block_and_defaults_to_deployment() {
        let (rpc, client) = client().await;
        rpc.push_success(&U64::from(client.max_block + 1));
        assert_eq!(client.provider.get_block_number().await.unwrap(), client.max_block + 1);
        let mut block = alloy_rpc_types::Block::<alloy_rpc_types::Transaction>::default();
        block.header.number = client.max_block;
        block.header.hash = B256::repeat_byte(1);
        rpc.push_success(&block);
        rpc.push_success(&Bytes::from(Address::repeat_byte(3).abi_encode()));
        rpc.push_success(&Vec::<Log>::new()); // No previous collection.
        rpc.push_success(&Vec::<Log>::new()); // No savings since deployment.
        rpc.push_success(&block);
        let fees = client.calculate().await.unwrap();
        assert_eq!(fees.block.number, client.max_block);
        assert!(fees.tokens.is_empty());
        assert!(rpc.read_q().is_empty());
    }
}
