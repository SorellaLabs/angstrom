use std::collections::{
    hash_map::{Entry, OccupiedEntry},
    HashMap, HashSet
};

use guard_types::{
    consensus::{Bundle23Votes, BundleVote, GuardSet, Valid23Bundle},
    on_chain::SimmedBundle
};
use reth_primitives::H256;
use tracing::{debug, error, warn};

/// The bundle vote manager is in-charge for tracking all bundle votes
/// in order to make sure that we are able to reach consensus on the best
/// bundle
pub struct BundleVoteManager {
    best_bundle:        Option<Valid23Bundle>,
    known_bundles:      HashMap<H256, SimmedBundle>,
    known_bundle_votes: HashMap<H256, Vec<BundleVote>>,
    known_23_bundles:   HashSet<H256>
}

impl Default for BundleVoteManager {
    fn default() -> Self {
        Self { ..Default::default() }
    }
}

impl BundleVoteManager {
    pub fn is_best_bundle(&self, bundle: &SimmedBundle) -> bool {
        let Some(our_best) = self.best_bundle.as_ref() else { return false };

        return our_best.bundle.get_cumulative_lp_bribe() == bundle.get_cumulative_lp_bribe()
    }

    pub fn new_simmed_bundle(&mut self, bundle: SimmedBundle) -> Option<H256> {
        let hash = bundle.hash();
        if self.known_23_bundles.contains(&hash) {
            return None
        }

        if self.known_bundles.insert(hash, bundle).is_none() {
            return Some(hash)
        }

        None
    }

    pub fn new_bundle23(&mut self, bundle: Valid23Bundle, guards: &GuardSet) -> bool {
        if !bundle.votes.verify_signatures(&guards) {
            warn!(?bundle, "bundle was invalid 2/3");
            return false
        }
        let hash = bundle.votes.hash;
        let new = !self.known_23_bundles.insert(hash);

        let _ = self.known_bundles.remove(&hash);

        if let Some(best_bundle) = self.best_bundle.take() {
            if bundle.bundle.get_cumulative_lp_bribe()
                > best_bundle.bundle.get_cumulative_lp_bribe()
            {
                self.best_bundle = Some(bundle);
            }
        } else {
            self.best_bundle = Some(bundle);
        }

        new
    }

    pub fn new_bundle_vote(
        &mut self,
        vote: BundleVote,
        guards: &GuardSet
    ) -> Option<Valid23Bundle> {
        let hash = vote.hash;
        if let Some(new_23) = match self.known_bundle_votes.entry(hash) {
            Entry::Vacant(v) => {
                if !Self::verify_vote(guards, &vote) {
                    return None
                }

                let mut entry = Vec::with_capacity(guards.len());
                entry.push(vote);
                v.insert(entry);
                None
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
                if !Self::verify_vote(guards, &vote) {
                    return None
                }
                o.get_mut().push(vote);

                Self::check_for_23(o, guards)
            }
        } {
            let hash = new_23.hash;
            let _ = self.known_bundle_votes.remove(&hash);
            let data = self.known_bundles.remove(&hash)?;
            self.known_23_bundles.insert(hash);

            let bribe = data.get_cumulative_lp_bribe();
            let built = Valid23Bundle { bundle: data, votes: new_23 };

            if let Some(cur_23) = self.best_bundle.take() {
                if cur_23.bundle.get_cumulative_lp_bribe() < bribe {
                    self.best_bundle = Some(built.clone());
                }
            }

            return Some(built)
        }

        None
    }

    pub fn contains_vote(&self, vote: &BundleVote) -> bool {
        self.known_bundle_votes
            .get(&vote.bundle_hash)
            .map(|votes| votes.contains(vote))
            .filter(|f| *f)
            .is_some()
    }

    pub fn has_signed_bundle(&self, bundle_hash: &H256) -> bool {
        self.known_bundle_votes.contains_key(bundle_hash)
    }

    fn verify_vote(guards: &GuardSet, vote: &BundleVote) -> bool {
        let Ok(id) = vote.recover_public_key()
        // .inspect_err(|e| error!(?e, "failed to recover vote"))
        else {
            return false
        };

        if !guards.contains_key(id) {
            warn!(?vote, "no guard found for recovered signature");
            return false
        }

        true
    }

    fn check_for_23(
        mut entry: OccupiedEntry<'_, H256, Vec<BundleVote>>,
        guards: &GuardSet
    ) -> Option<Bundle23Votes> {
        let total_guards = guards.len();
        // check to see if we have less than 2/3rd
        if entry.get().len() % total_guards <= 66 {
            return None
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

        Some(new_bundle_votes)
    }
}
