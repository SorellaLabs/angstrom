use std::{future::Future, pin::Pin, task::Poll};

use alloy::{
    network::{Ethereum, EthereumWallet},
    node_bindings::{Anvil, AnvilInstance},
    providers::{Network, Provider, builder, ext::AnvilApi},
    rpc::types::{Block, anvil::MineOptions},
    signers::local::PrivateKeySigner
};
use alloy_primitives::Bytes;
use alloy_rpc_types::{Header, Transaction};
use angstrom_types::primitive::CHAIN_ID;
use futures::{Stream, StreamExt, stream::FuturesOrdered};
use reth_node_types::NodePrimitives;
use reth_primitives::EthPrimitives;

use super::{
    AnvilStateProvider, WalletProvider,
    compat::{rpc_block_to_pr_block, rpc_receipts_to_pr_receipts}
};
use crate::{
    contracts::anvil::WalletProviderRpc,
    providers::StartMonitor,
    types::{WithWalletProvider, initial_state::DeployedAddresses}
};

#[derive(Debug)]
pub struct AnvilProvider<P, N: Network = Ethereum, PR: NodePrimitives = EthPrimitives> {
    provider:           AnvilStateProvider<P, N, PR>,
    deployed_addresses: Option<DeployedAddresses>,
    pub _instance:      Option<AnvilInstance>
}

impl<P, N, PR> AnvilProvider<P, N, PR>
where
    P: WithWalletProvider,
    N: Network,
    PR: NodePrimitives
{
    pub fn new(
        provider: AnvilStateProvider<P, N, PR>,
        anvil: Option<AnvilInstance>,
        deployed_addresses: Option<DeployedAddresses>
    ) -> Self {
        Self { provider, _instance: anvil, deployed_addresses }
    }

    pub fn deployed_addresses(&self) -> Option<DeployedAddresses> {
        self.deployed_addresses
    }

    pub fn into_state_provider(&mut self) -> AnvilProvider<WalletProvider, N, PR> {
        AnvilProvider {
            provider:           self.provider.as_wallet_state_provider(),
            deployed_addresses: self.deployed_addresses,
            _instance:          self._instance.take()
        }
    }

    pub fn state_provider(&self) -> AnvilStateProvider<WalletProvider, N, PR> {
        self.provider.as_wallet_state_provider()
    }

    pub fn wallet_provider(&self) -> WalletProvider {
        self.provider.provider().wallet_provider()
    }

    pub fn rpc_provider(&self) -> WalletProviderRpc {
        self.provider.provider().rpc_provider()
    }

    pub fn provider(&self) -> &AnvilStateProvider<P, N, PR> {
        &self.provider
    }

    pub fn provider_mut(&mut self) -> &mut AnvilStateProvider<P, N, PR> {
        &mut self.provider
    }

    pub async fn execute_and_return_state(&self) -> eyre::Result<(Bytes, Block)>
    where
        PR::Block: TryFrom<alloy_rpc_types::Block>,
        <PR::Block as TryFrom<alloy_rpc_types::Block>>::Error: std::fmt::Debug,
        PR::Receipt: TryFrom<alloy_rpc_types::ReceiptEnvelope<alloy_rpc_types::Log>>,
        <PR::Receipt as TryFrom<alloy_rpc_types::ReceiptEnvelope<alloy_rpc_types::Log>>>::Error:
            std::fmt::Debug
    {
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

    pub async fn mine_block(&self) -> eyre::Result<Block>
    where
        PR::Block: TryFrom<alloy_rpc_types::Block>,
        <PR::Block as TryFrom<alloy_rpc_types::Block>>::Error: std::fmt::Debug,
        PR::Receipt: TryFrom<alloy_rpc_types::ReceiptEnvelope<alloy_rpc_types::Log>>,
        <PR::Receipt as TryFrom<alloy_rpc_types::ReceiptEnvelope<alloy_rpc_types::Log>>>::Error:
            std::fmt::Debug
    {
        let mined = self
            .provider
            .provider()
            .rpc_provider()
            .anvil_mine_detailed(Some(MineOptions::Options { timestamp: None, blocks: Some(1) }))
            .await?
            .first()
            .cloned()
            .unwrap();

        let number = mined.header.number;
        let recipts = self
            .provider
            .provider()
            .rpc_provider()
            .get_block_receipts(alloy_rpc_types::BlockId::number(number))
            .await
            .unwrap()
            .unwrap();

        let pr_mined = rpc_block_to_pr_block::<PR>(&mined)?;
        let pr_receipts = rpc_receipts_to_pr_receipts::<PR>(recipts)?;

        self.provider.update_canon_chain(&pr_mined, pr_receipts)?;

        Ok(mined)
    }

    pub async fn subscribe_blocks(
        &self
    ) -> eyre::Result<impl Stream<Item = (u64, Vec<Transaction>)> + Unpin + Send + 'static> {
        let stream = self.rpc_provider().subscribe_blocks().await?.into_stream();

        Ok(StreamBlockProvider::new(self.rpc_provider(), stream))
    }
}

impl AnvilProvider<WalletProvider, Ethereum, EthPrimitives> {
    pub async fn spawn_new_isolated() -> eyre::Result<Self> {
        let anvil = Anvil::new()
            .block_time(12)
            .chain_id(*CHAIN_ID.get().unwrap())
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
            .connect(ipc)
            .await?;

        tracing::info!("connected to anvil");

        Ok(Self {
            provider:           AnvilStateProvider::new(WalletProvider::new_with_provider(rpc, sk)),
            _instance:          Some(anvil),
            deployed_addresses: None
        })
    }
}

impl<P, N, PR> AnvilProvider<P, N, PR>
where
    PR: NodePrimitives,
    N: Network,
    P: WithWalletProvider,
    AnvilStateProvider<WalletProvider, N, PR>: StartMonitor
{
    pub async fn from_future<F>(fut: F, testnet: bool) -> eyre::Result<Self>
    where
        F: std::future::Future<
                Output = eyre::Result<(P, Option<AnvilInstance>, Option<DeployedAddresses>)>
            >
    {
        let (provider, anvil, deployed_addresses) = fut.await?;
        let this = Self {
            provider: AnvilStateProvider::new(provider),
            _instance: anvil,
            deployed_addresses
        };
        if testnet {
            let sp = this.provider.as_wallet_state_provider();
            StartMonitor::spawn(sp);
        }
        Ok(this)
    }
}

struct StreamBlockProvider {
    provider:      WalletProviderRpc,
    header_stream: Pin<Box<dyn Stream<Item = Header> + Send>>,
    futs:          FuturesOrdered<Pin<Box<dyn Future<Output = (u64, Vec<Transaction>)> + Send>>>
}

impl StreamBlockProvider {
    fn new(
        provider: WalletProviderRpc,
        header_stream: impl Stream<Item = Header> + Send + 'static
    ) -> Self {
        Self { provider, header_stream: Box::pin(header_stream), futs: FuturesOrdered::new() }
    }

    fn new_block(&mut self, header: Header) {
        self.futs
            .push_back(Box::pin(Self::make_block(self.provider.clone(), header.number)));
    }

    async fn make_block(provider: WalletProviderRpc, number: u64) -> (u64, Vec<Transaction>) {
        let block = provider
            .get_block(number.into())
            .full()
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

        while let Poll::Ready(Some(header)) = this.header_stream.poll_next_unpin(cx) {
            this.new_block(header);
        }

        if let Poll::Ready(Some(val)) = this.futs.poll_next_unpin(cx) {
            return Poll::Ready(Some(val));
        }

        Poll::Pending
    }
}
