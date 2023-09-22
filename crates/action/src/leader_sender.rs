use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll}
};

use ethers_core::types::transaction::eip2718::TypedTransaction;
use ethers_flashbots::{BroadcasterMiddleware, BundleRequest, PendingBundleError};
use ethers_middleware::SignerMiddleware;
use ethers_providers::Middleware;
use ethers_signers::{LocalWallet, Signer};
use futures::{Future, FutureExt};
use guard_types::on_chain::{BundleSignature, SimmedBundle};
use reth_primitives::PeerId;
use tracing::{debug, info};

type StakedWallet = LocalWallet;
type BundleKey = LocalWallet;

pub type SubmissionFut = Pin<Box<dyn Future<Output = Result<(), PendingBundleError>> + Send>>;

pub struct LeaderSender<M: Middleware + 'static> {
    signer: Arc<SignerMiddleware<BroadcasterMiddleware<&'static M, BundleKey>, StakedWallet>>,
    signatures:    Vec<BundleSignature>,
    valid_stakers: Vec<PeerId>,
    bundle:        Option<SimmedBundle>,
    future:        Option<SubmissionFut>
}

impl<M: Middleware + 'static> LeaderSender<M> {
    pub fn new(
        signer: Arc<SignerMiddleware<BroadcasterMiddleware<&'static M, BundleKey>, StakedWallet>>
    ) -> Self {
        Self {
            signer,
            signatures: Vec::new(),
            valid_stakers: Vec::new(),
            bundle: None,
            future: None
        }
    }

    pub fn has_submitted(&self) -> bool {
        self.future.is_some()
    }

    pub fn submit_bundle(&mut self, bundle: SimmedBundle) {
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
                        Ok(bundle_hash) => println!(
                            "Bundle with hash {:?} was included in target block",
                            bundle_hash
                        ),
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

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        if let Some(submit_fut) = self.future.as_mut() {
            return submit_fut.poll_unpin(cx).map(|e| e.unwrap())
        }

        Poll::Pending
    }
}
