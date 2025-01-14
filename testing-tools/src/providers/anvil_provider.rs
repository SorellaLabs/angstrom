use std::{future::Future, pin::Pin, task::Poll};

use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::{Anvil, AnvilInstance},
    providers::{builder, ext::AnvilApi, Provider},
    rpc::types::{anvil::MineOptions, Block},
    signers::local::PrivateKeySigner
};
use alloy_primitives::Bytes;
use alloy_rpc_types::{BlockTransactionsKind, Header, Transaction};
use angstrom_types::block_sync::GlobalBlockSync;
use futures::{stream::FuturesUnordered, Stream, StreamExt};

use super::{AnvilStateProvider, WalletProvider};
use crate::{contracts::anvil::WalletProviderRpc, types::WithWalletProvider};

#[derive(Debug)]
pub struct AnvilProvider<P> {
    provider:      AnvilStateProvider<P>,
    pub _instance: Option<AnvilInstance>
}
impl<P> AnvilProvider<P>
where
    P: WithWalletProvider
{
    pub async fn new<F>(fut: F, testnet: bool, block_sync: GlobalBlockSync) -> eyre::Result<Self>
    where
        F: Future<Output = eyre::Result<(P, Option<AnvilInstance>)>>
    {
        let (provider, anvil) = fut.await?;
        let this =
            Self { provider: AnvilStateProvider::new(provider, block_sync), _instance: anvil };
        if testnet {
            tokio::spawn(
                this.provider
                    .as_wallet_state_provider()
                    .listen_to_new_blocks()
            );
        }
        Ok(this)
    }

    pub fn into_state_provider(&mut self) -> AnvilProvider<WalletProvider> {
        AnvilProvider {
            provider:  self.provider.as_wallet_state_provider(),
            _instance: self._instance.take()
        }
    }

    pub fn state_provider(&self) -> AnvilStateProvider<WalletProvider> {
        self.provider.as_wallet_state_provider()
    }

    pub fn wallet_provider(&self) -> WalletProvider {
        self.provider.provider().wallet_provider()
    }

    pub fn rpc_provider(&self) -> WalletProviderRpc {
        self.provider.provider().rpc_provider()
    }

    pub fn provider(&self) -> &AnvilStateProvider<P> {
        &self.provider
    }

    pub fn provider_mut(&mut self) -> &mut AnvilStateProvider<P> {
        &mut self.provider
    }

    pub async fn execute_and_return_state(&self) -> eyre::Result<(Bytes, Block)> {
        let block = self.mine_block().await?;

        Ok((
            self.provider
                .provider()
                .rpc_provider()
                .anvil_dump_state()
                .await?,
            block
        ))
    }

    pub async fn return_state(&self) -> eyre::Result<Bytes> {
        Ok(self
            .provider
            .provider()
            .rpc_provider()
            .anvil_dump_state()
            .await?)
    }

    pub async fn set_state(&self, state: Bytes) -> eyre::Result<()> {
        self.provider
            .provider()
            .rpc_provider()
            .anvil_load_state(state)
            .await?;

        Ok(())
    }

    pub async fn mine_block(&self) -> eyre::Result<Block> {
        let mined = self
            .provider
            .provider()
            .rpc_provider()
            .anvil_mine_detailed(Some(MineOptions::Options { timestamp: None, blocks: Some(1) }))
            .await?
            .first()
            .cloned()
            .unwrap();

        self.provider.update_canon_chain(&mined)?;

        Ok(mined)
    }

    pub async fn subscribe_blocks(
        &self
    ) -> eyre::Result<impl Stream<Item = (u64, Vec<Transaction>)> + Unpin + Send> {
        let stream = self.rpc_provider().subscribe_blocks().await?.into_stream();

        Ok(StreamBlockProvider::new(self.rpc_provider(), stream))
    }
}

impl AnvilProvider<WalletProvider> {
    pub async fn spawn_new_isolated(block_sync: GlobalBlockSync) -> eyre::Result<Self> {
        let anvil = Anvil::new()
            .block_time(12)
            .chain_id(1)
            .arg("--ipc")
            .arg("--code-size-limit")
            .arg("393216")
            .arg("--disable-block-gas-limit")
            .try_spawn()?;

        let ipc = "/tmp/anvil.ipc";
        let sk: PrivateKeySigner = anvil.keys()[7].clone().into();

        let wallet = EthereumWallet::new(sk.clone());
        let rpc = builder::<Ethereum>()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_builtin(ipc)
            .await?;

        tracing::info!("connected to anvil");

        Ok(Self {
            provider:  AnvilStateProvider::new(
                WalletProvider::new_with_provider(rpc, sk),
                block_sync
            ),
            _instance: Some(anvil)
        })
    }
}

struct StreamBlockProvider {
    provider:      WalletProviderRpc,
    header_stream: Pin<Box<dyn Stream<Item = Header> + Send>>,
    futs:          FuturesUnordered<Pin<Box<dyn Future<Output = (u64, Vec<Transaction>)> + Send>>>
}

impl StreamBlockProvider {
    fn new(
        provider: WalletProviderRpc,
        header_stream: impl Stream<Item = Header> + Send + 'static
    ) -> Self {
        Self { provider, header_stream: Box::pin(header_stream), futs: FuturesUnordered::new() }
    }

    fn new_block(&mut self, header: Header) {
        self.futs
            .push(Box::pin(Self::make_block(self.provider.clone(), header.number)));
    }

    async fn make_block(provider: WalletProviderRpc, number: u64) -> (u64, Vec<Transaction>) {
        let block = provider
            .get_block(number.into(), BlockTransactionsKind::Full)
            .await
            .unwrap_or_else(|_| panic!("could not get block number {number}"))
            .unwrap_or_else(|| panic!("no block found - number {number}"));

        (number, block.transactions.into_transactions().collect())
    }
}

impl Stream for StreamBlockProvider {
    type Item = (u64, Vec<Transaction>);

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if let Poll::Ready(Some(header)) = this.header_stream.poll_next_unpin(cx) {
            this.new_block(header);
        }

        if let Poll::Ready(Some(val)) = this.futs.poll_next_unpin(cx) {
            return Poll::Ready(Some(val))
        }

        Poll::Pending
    }
}
