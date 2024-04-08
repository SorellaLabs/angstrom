use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use angstrom_types::submission::SubmissionBundle;
use ethers_core::types::transaction::eip2718::TypedTransaction;
use ethers_flashbots::{BroadcasterMiddleware, BundleRequest, PendingBundleError};
use ethers_middleware::SignerMiddleware;
use ethers_providers::Middleware;
use ethers_signers::{LocalWallet, Signer};
use futures::{Future, FutureExt};
use tokio::sync::{mpsc, mpsc::UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;
type StakedWallet = LocalWallet;
type BundleKey = LocalWallet;

pub type SubmissionFut = Pin<Box<dyn Future<Output = Result<(), PendingBundleError>> + Send>>;

pub enum SubmissionCommand {
    SubmitBundle(SubmissionBundle)
}

/// Api to interact with [`TransactionsManager`] task.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SubmissionHandle {
    /// Command channel to the [`TransactionsManager`]
    manager_tx: mpsc::UnboundedSender<SubmissionCommand>
}

pub struct RelaySender<M>
where
    M: Middleware + 'static
{
    signer:     Arc<SignerMiddleware<BroadcasterMiddleware<&'static M, BundleKey>, StakedWallet>>,
    future:     Option<SubmissionFut>,
    command_tx: UnboundedSender<SubmissionCommand>,
    #[allow(dead_code)]
    command_rx: UnboundedReceiverStream<SubmissionCommand>
}

impl<M: Middleware + 'static> RelaySender<M> {
    pub fn new(
        signer: Arc<SignerMiddleware<BroadcasterMiddleware<&'static M, BundleKey>, StakedWallet>>
    ) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        Self {
            signer,
            future: None,
            command_tx,
            command_rx: UnboundedReceiverStream::new(command_rx)
        }
    }

    pub fn handle(&self) -> SubmissionHandle {
        SubmissionHandle { manager_tx: self.command_tx.clone() }
    }

    pub fn has_submitted(&self) -> bool {
        self.future.is_some()
    }

    pub fn submit_bundle(&mut self, _bundle: SubmissionBundle) {
        let client = self.signer.clone();

        self.future = Some(Box::pin(async move {
            let block_number = client.get_block_number().await.unwrap();

            let tx = {
                let mut tx = TypedTransaction::default();
                //TODO: add calldata encyption
                let access_list = client.create_access_list(&tx, None).await.unwrap();

                tx.set_from(client.address())
                    .set_access_list(access_list.access_list);
                tx
            };
            let signature = client.signer().sign_transaction(&tx).await.unwrap();

            let bundle = BundleRequest::new()
                .push_transaction(tx.rlp_signed(&signature))
                .set_block(block_number + 1)
                .set_simulation_block(block_number)
                .set_simulation_timestamp(0);

            // Send it
            let results = client.inner().send_bundle(&bundle).await.unwrap();

            // You can also optionally wait to see if the bundle was included
            for result in results {
                match result {
                    Ok(pending_bundle) => match pending_bundle.await {
                        Ok(bundle_hash) => {
                            println!(
                                "Bundle with hash {:?} was included in target block",
                                bundle_hash
                            )
                        }
                        Err(PendingBundleError::BundleNotIncluded) => {
                            println!("Bundle was not included in target block.")
                        }
                        Err(e) => println!("An error occured: {}", e)
                    },
                    Err(e) => println!("An error occured: {}", e)
                }
            }

            Ok(())
        }));
    }

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), PendingBundleError>> {
        self.future
            .as_mut()
            .map(|fut| fut.poll_unpin(cx))
            .unwrap_or(Poll::Pending)
    }
}
