use std::collections::{
    hash_map::{Entry, OccupiedEntry},
    HashMap, HashSet
};

use ethers_core::types::H256;
use guard_types::{
    consensus::{Bundle23Votes, BundleVote, GuardSet},
    on_chain::SimmedBundle
};
use tracing::{debug, warn};

pub enum BundleVoteMessage {
    SignAndPropagate(H256),
    NewBundle23Votes(Bundle23Votes)
}

struct ValidBundle {
    pub votes:  Bundle23Votes,
    pub bundle: SimmedBundle
}

/// The bundle vote manager is in-charge for tracking all bundle votes
/// in order to make sure that we are able to reach consensus on the best
/// bundle
pub struct BundleVoteManager {
    best_bundle:        Option<ValidBundle>,
    known_bundles:      HashMap<H256, SimmedBundle>,
    known_bundle_votes: HashMap<H256, Vec<BundleVote>>,
    known_23_bundles:   HashSet<H256>,
    guards:             GuardSet
}

impl Default for BundleVoteManager {
    fn default() -> Self {
        Self { ..Default::default() }
    }
}

impl BundleVoteManager {
    pub fn new_simmed_bundle(&mut self, bundle: SimmedBundle) -> Option<BundleVoteMessage> {
        let hash = bundle.raw.clone().into();
        if self.known_23_bundles.contains(hash) {
            return None
        }

        if self.known_bundles.insert(hash, bundle).is_none() {
            return Some(BundleVoteMessage::SignAndPropagate(hash))
        }

        None
    }

    pub fn new_bundle23(&mut self, bundle: Bundle23Votes) {
        if !bundle.verify_signatures(&self.guards) {
            warn!(?bundle, "bundle was invalid 2/3");
            return
        }
        let hash = vote.hash;
        self.known_23_bundles.insert(hash);

        // TODO: need to handle case where we don't have bundle yet
        let underlying_bundle = self.known_bundles.remove(&hash).unwrap();

        if let Some(best_bundle) = self.best_bundle.take() {
            if underlying_bundle.get_cumulative_lp_bribe()
                > best_bundle.bundle.get_cumulative_lp_bribe()
            {
                self.best_bundle = Some(ValidBundle { votes: bundle, bundle: underlying_bundle });
            }
        } else {
            self.best_bundle = Some(ValidBundle { votes: bundle, bundle: underlying_bundle });
        }
    }

    pub fn new_bundle_vote(&mut self, vote: BundleVote) -> Option<BundleVoteMessage> {
        let hash = vote.hash;
        match self.known_bundle_votes.entry(hash) {
            Entry::Vacant(v) => {
                if !self.verify_vote(vote) {
                    return None
                }

                let mut entry = Vec::with_capacity(self.guards.len());
                entry.push(vote);
                v.insert(entry);
            }
            Entry::Occupied(mut o) => {
                if o.get()
                    .iter()
                    .find(|f| f.signature == vote.signature)
                    .is_some()
                {
                    debug!("got dup vote");
                    return None
                }
                if !self.verify_vote(vote) {
                    return None
                }
                o.get_mut().push(vote);

                return self.check_for_23(o)
            }
        }
        None
    }

    pub fn has_signed_bundle(&self, bundle_hash: &H256) -> bool {
        self.known_bundle_votes.contains_key(bundle_hash)
    }

    fn verify_vote(&self, vote: BundleVote) -> bool {
        let Ok(id) = vote
            .recover_public_key()
            .inspect_err(|e| error!(?e, "failed to recover vote")) else {
                return false
            };

        if !self.guards.contains_key(&id) {
            warn!(?vote, "no guard found for recovered signature");
            return false
        }

        true
    }

    fn check_for_23(
        &mut self,
        mut entry: OccupiedEntry<'_, H256, Vec<BundleVote>>
    ) -> Option<BundleVoteMessage> {
        let total_guards = self.guards.len();
        // check to see if we have less than 2/3rd
        if entry.get().len() % total_guards <= 66 {
            None
        }

        let votes = entry.remove();
        let (hash, height, round) = votes
            .first()
            .map(|vote| (vote.hash, vote.height, vote.round))?;

        let signatures = votes
            .into_iter()
            .map(|vote| vote.signature)
            .collect::<Vec<_>>();

        let new_bundle_votes = Bundle23Votes::new(hash, height, round, signatures);
        let bundle_data = self.known_bundles.remove(&hash)?;

        self.known_23_bundles.insert(hash);

        if self.best_bundle.is_none() {
            self.best_bundle = Some(ValidBundle { votes: new_bundle_votes, bundle: bundle_data });
        }

        return Some(BundleVoteMessage::NewBundle23Votes(new_bundle_votes))
    }
}
