use std::{cmp::Ordering, collections::HashSet};

use alloy::primitives::BlockNumber;
use angstrom_types::primitive::PeerId;

// https://github.com/tendermint/tendermint/pull/2785#discussion_r235038971
// 1.125
const PENALTY_FACTOR: u64 = 1125;
/// do the math with fixed here to avoid floats
const ONE_E3: u64 = 1000;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AngstromValidator {
    pub peer_id:  PeerId,
    voting_power: u64,
    priority:     i64
}

impl AngstromValidator {
    pub fn new(name: PeerId, voting_power: u64) -> Self {
        AngstromValidator {
            peer_id:      name,
            voting_power: voting_power * ONE_E3,
            priority:     0
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct WeightedRoundRobin {
    validators:                HashSet<AngstromValidator>,
    new_joiner_penalty_factor: u64,
    block_number:              BlockNumber,
    last_proposer:             Option<PeerId>
}

impl WeightedRoundRobin {
    pub fn new(validators: Vec<AngstromValidator>, block_number: BlockNumber) -> Self {
        WeightedRoundRobin {
            validators: HashSet::from_iter(validators),
            new_joiner_penalty_factor: PENALTY_FACTOR,
            block_number,
            last_proposer: None
        }
    }

    fn proposer_selection(&mut self) -> Option<PeerId> {
        let total_voting_power: u64 = self.validators.iter().map(|v| v.voting_power).sum();

        //  apply all priorities.
        self.validators = self
            .validators
            .drain()
            .map(|mut validator| {
                validator.priority += validator.voting_power as i64;
                validator
            })
            .collect();

        // find the max
        let mut proposer = self.validators.iter().max_by(Self::priority)?.clone();

        proposer.priority -= total_voting_power as i64;
        let proposer_name = proposer.peer_id;

        self.validators.replace(proposer);

        Some(proposer_name)
    }

    fn reverse_proposer_selection(&mut self) -> Option<PeerId> {
        let total_voting_power: u64 = self.validators.iter().map(|v| v.voting_power).sum();

        // grab the last proposeer
        let mut proposer = self.validators.iter().min_by(Self::priority)?.clone();

        // undo the total_voting_power subtraction
        proposer.priority += total_voting_power as i64;
        let proposer_name = proposer.peer_id;

        // undo the priority additions for all validators first
        self.validators = self
            .validators
            .drain()
            .map(|mut validator| {
                validator.priority -= validator.voting_power as i64;
                validator
            })
            .collect();

        // then replace the proposer with updated priority
        self.validators.replace(proposer);

        Some(proposer_name)
    }

    fn priority(a: &&AngstromValidator, b: &&AngstromValidator) -> Ordering {
        let out = a.priority.partial_cmp(&b.priority);
        if out == Some(Ordering::Equal) {
            // TODO: not the best because it encourages mining lower peer ids
            // however we need a way for this to be uniform across nodes and
            // this is the easiest
            return a.peer_id.cmp(&b.peer_id)
        }
        out.unwrap()
    }

    fn center_priorities(&mut self) {
        if self.validators.is_empty() {
            tracing::error!("no validators are set");
            return
        }

        let avg_priority =
            self.validators.iter().map(|v| v.priority).sum::<i64>() / self.validators.len() as i64;

        self.validators = self
            .validators
            .drain()
            .map(|mut validator| {
                validator.priority -= avg_priority;
                validator
            })
            .collect();
    }

    fn uncenter_priorities(&mut self, target_avg: i64) {
        if self.validators.is_empty() {
            tracing::error!("no validators are set");
            return
        }

        self.validators = self
            .validators
            .drain()
            .map(|mut validator| {
                validator.priority += target_avg;
                validator
            })
            .collect();
    }

    fn scale_priorities(&mut self) {
        if self.validators.is_empty() {
            tracing::error!("no validators are set");
            return
        }

        let max_priority = self
            .validators
            .iter()
            .map(|v| v.priority)
            .fold(i64::MIN, i64::max);
        let min_priority = self
            .validators
            .iter()
            .map(|v| v.priority)
            .fold(i64::MAX, i64::min);

        let total_voting_power: u64 = self.validators.iter().map(|v| v.voting_power).sum();
        let diff = max_priority - min_priority;
        let threshold = 2 * total_voting_power as i64;

        if diff > threshold {
            let scale = (diff * ONE_E3 as i64) / threshold;

            self.validators = self
                .validators
                .drain()
                .map(|mut validator| {
                    let new_pri = validator.priority * ONE_E3 as i64;
                    validator.priority = new_pri / scale;
                    validator
                })
                .collect();
        }
    }

    fn unscale_priorities(&mut self) -> i64 {
        if self.validators.is_empty() {
            tracing::error!("no validators are set");
            return 0
        }

        let max_priority = self
            .validators
            .iter()
            .map(|v| v.priority)
            .fold(i64::MIN, i64::max);

        let min_priority = self
            .validators
            .iter()
            .map(|v| v.priority)
            .fold(i64::MAX, i64::min);

        let total_voting_power: u64 = self.validators.iter().map(|v| v.voting_power).sum();
        let diff = max_priority - min_priority;
        let threshold = 2 * total_voting_power as i64;

        if diff > threshold {
            let scale = (diff * ONE_E3 as i64) / threshold;

            self.validators = self
                .validators
                .drain()
                .map(|mut validator| {
                    // validator.priority = (validator.priority * ONE_E3 as i64) / scale;
                    // p = (x * c) / s
                    validator.priority = (validator.priority * scale) / (ONE_E3 as i64);
                    validator
                })
                .collect();
        }

        (self
            .validators
            .iter()
            .map(|v| v.priority as i128)
            .sum::<i128>()
            / self.validators.len() as i128) as i64
    }

    // pub fn choose_proposer(&mut self, block_number: BlockNumber) ->
    // Option<PeerId> {     if block_number <= self.block_number {
    //         if self.last_proposer.is_none() {
    //             self.last_proposer = Some(self.proposer_selection()?);
    //         }
    //
    //         return self.last_proposer
    //     }
    //
    //     let rounds_to_catchup = (block_number - self.block_number) as usize;
    //     let mut leader = None;
    //     for _ in 0..rounds_to_catchup {
    //         self.center_priorities();
    //         self.scale_priorities();
    //         leader = Some(self.proposer_selection()?);
    //         self.last_proposer = leader
    //     }
    //     self.block_number = block_number;
    //     leader
    // }

    pub fn choose_proposer(&mut self, block_number: BlockNumber) -> Option<PeerId> {
        if block_number == self.block_number {
            if self.last_proposer.is_none() {
                self.last_proposer = Some(self.proposer_selection()?);
            }
            return self.last_proposer;
        }

        // Reset state to handle both forward and backward transitions consistently
        let mut leader = None;
        let target_block = block_number;

        // Start from a known state
        let mut current_block = self.block_number;

        while current_block != target_block {
            if current_block < target_block {
                self.center_priorities();
                self.scale_priorities();
                self.last_proposer = Some(self.proposer_selection()?);
                current_block += 1;
            } else {
                let target_avg = self.unscale_priorities();
                self.uncenter_priorities(target_avg);
                self.last_proposer = Some(self.reverse_proposer_selection()?);
                current_block -= 1;
            }
            leader = Some(self.proposer_selection()?);
            self.last_proposer = leader;
        }

        self.block_number = block_number;
        leader
    }

    #[allow(dead_code)]
    fn remove_validator(&mut self, peer_id: &PeerId) {
        let validator = AngstromValidator::new(*peer_id, 0);
        self.validators.remove(&validator);
    }

    #[allow(dead_code)]
    fn add_validator(&mut self, peer_id: PeerId, voting_power: u64) {
        let mut new_validator = AngstromValidator::new(peer_id, voting_power);
        let total_voting_power: u64 = self.validators.iter().map(|v| v.voting_power).sum();
        new_validator.priority -=
            ((self.new_joiner_penalty_factor * total_voting_power) / ONE_E3) as i64;
        self.validators.insert(new_validator);
    }
}

impl PartialEq for AngstromValidator {
    fn eq(&self, other: &Self) -> bool {
        self.peer_id == other.peer_id
    }
}

impl Eq for AngstromValidator {}

impl std::hash::Hash for AngstromValidator {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.peer_id.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use alloy::primitives::BlockNumber;

    use super::*;

    fn create_test_validators() -> (HashMap<String, PeerId>, Vec<AngstromValidator>) {
        let peers = HashMap::from([
            ("Alice".to_string(), PeerId::random()),
            ("Bob".to_string(), PeerId::random()),
            ("Charlie".to_string(), PeerId::random())
        ]);
        let validators = vec![
            AngstromValidator::new(peers["Alice"], 100),
            AngstromValidator::new(peers["Bob"], 200),
            AngstromValidator::new(peers["Charlie"], 300),
        ];
        (peers, validators)
    }

    #[test]
    fn test_priority_calculation() {
        let (_, validators) = create_test_validators();
        let mut algo = WeightedRoundRobin::new(validators, BlockNumber::default());

        // Get initial priorities
        let initial_priorities: Vec<i64> = algo.validators.iter().map(|v| v.priority).collect();
        assert!(initial_priorities.iter().all(|&p| p == 0), "Initial priorities should be 0");

        // Record initial voting powers
        let initial_powers: Vec<u64> = algo.validators.iter().map(|v| v.voting_power).collect();

        // Test single round of priority updates
        algo.proposer_selection();

        // After proposer selection:
        // 1. All validators should have their priority increased by their voting power
        // 2. The selected validator (highest priority) should then have total_power
        //    subtracted
        let total_power: u64 = initial_powers.iter().sum();

        for validator in algo.validators.iter() {
            if validator.priority < 0 {
                // This was the selected validator
                let expected_priority = validator.voting_power as i64 - total_power as i64;
                assert_eq!(
                    validator.priority, expected_priority,
                    "Selected validator should have priority = (own_power - total_power)"
                );
            } else {
                // Non-selected validators just got their voting power added
                assert_eq!(
                    validator.priority, validator.voting_power as i64,
                    "Non-selected validator should have priority = own_power"
                );
            }
        }
    }

    #[test]
    fn test_priority_centering() {
        let (_, validators) = create_test_validators();
        let mut algo = WeightedRoundRobin::new(validators, BlockNumber::default());

        // Set priorities to unscaled voting powers to test centering
        let max_power = 300; // Matches max voting power in create_test_validators
        algo.validators = algo
            .validators
            .drain()
            .map(|mut v| {
                // Use unscaled value to avoid massive numbers
                v.priority = (v.voting_power / ONE_E3) as i64;
                v
            })
            .collect();

        // Center priorities
        algo.center_priorities();

        // After centering:
        // 1. Sum should be close to zero (within rounding error)
        let sum_priorities: i64 = algo.validators.iter().map(|v| v.priority).sum();

        assert!(
            sum_priorities.abs() <= algo.validators.len() as i64,
            "Sum of centered priorities ({}) should be close to zero",
            sum_priorities
        );

        // 2. Each priority should be within reasonable bounds
        for validator in algo.validators.iter() {
            assert!(
                validator.priority.abs() <= max_power,
                "Individual priority ({}) should be within reasonable bounds",
                validator.priority
            );
        }

        // 3. Verify relative differences are maintained
        let priorities: Vec<i64> = algo.validators.iter().map(|v| v.priority).collect();
        let max_priority = priorities.iter().max().unwrap();
        let min_priority = priorities.iter().min().unwrap();
        assert!((max_priority - min_priority) <= max_power, "Priority spread should be reasonable");
    }

    #[test]
    fn test_priority_scaling() {
        let (_, validators) = create_test_validators();
        let mut algo = WeightedRoundRobin::new(validators, BlockNumber::default());

        // Set extreme priorities to trigger scaling
        let total_power: u64 = algo.validators.iter().map(|v| v.voting_power).sum();
        algo.validators = algo
            .validators
            .drain()
            .enumerate()
            .map(|(i, mut v)| {
                v.priority = (i as i64) * (total_power as i64) * 3; // Create large differences
                v
            })
            .collect();

        // Scale priorities
        algo.scale_priorities();

        // Verify scaling reduced the difference
        let max_priority = algo.validators.iter().map(|v| v.priority).max().unwrap();
        let min_priority = algo.validators.iter().map(|v| v.priority).min().unwrap();

        assert!(
            (max_priority - min_priority) <= 2 * (total_power as i64),
            "Priority difference should be less than twice the total voting power"
        );
    }

    #[test]
    fn test_proposer_selection_determinism() {
        let (_, validators) = create_test_validators();
        let mut algo1 = WeightedRoundRobin::new(validators.clone(), BlockNumber::default());
        let mut algo2 = WeightedRoundRobin::new(validators, BlockNumber::default());

        // Run multiple rounds and verify both instances select the same proposers
        for i in 1..=10 {
            let proposer1 = algo1.choose_proposer(i);
            let proposer2 = algo2.choose_proposer(i);
            assert_eq!(proposer1, proposer2, "Proposer selection should be deterministic");
        }
    }

    #[test]
    fn test_validator_management() {
        let (peers, validators) = create_test_validators();
        let mut algo = WeightedRoundRobin::new(validators, BlockNumber::default());

        // Test removing validator
        algo.remove_validator(&peers["Alice"]);
        assert_eq!(algo.validators.len(), 2);

        // Test adding validator
        let new_peer = PeerId::random();
        algo.add_validator(new_peer, 150);
        assert_eq!(algo.validators.len(), 3);

        // Verify new validator has penalty applied
        let new_validator = algo
            .validators
            .iter()
            .find(|v| v.peer_id == new_peer)
            .unwrap();
        assert!(
            new_validator.priority < 0,
            "New validator should have negative priority due to penalty"
        );
    }

    #[test]
    fn test_reorg_behavior() {
        let (_, validators) = create_test_validators();
        let mut algo = WeightedRoundRobin::new(validators, 10);

        // Normal progression
        let proposer1 = algo.choose_proposer(11);
        let proposer2 = algo.choose_proposer(12);
        assert!(proposer1.is_some());
        assert!(proposer2.is_some());

        // Reorg to lower block
        let reorg_proposer = algo.choose_proposer(11);
        assert_eq!(reorg_proposer, proposer1, "Should return same proposer on reorg");
    }

    #[test]
    fn test_empty_validator_set() {
        let mut algo = WeightedRoundRobin::new(vec![], BlockNumber::default());
        assert_eq!(algo.validators.len(), 0);

        // Should handle empty validator set gracefully
        let proposer = algo.choose_proposer(1);
        assert!(proposer.is_none());
    }

    #[test]
    fn test_single_validator() {
        let peer_id = PeerId::random();
        let validators = vec![AngstromValidator::new(peer_id, 100)];
        let mut algo = WeightedRoundRobin::new(validators, BlockNumber::default());

        // Single validator should always be chosen
        for i in 1..=5 {
            let proposer = algo.choose_proposer(i);
            assert_eq!(proposer, Some(peer_id));
        }
    }

    #[test]
    fn test_equal_voting_power() {
        let peers = HashMap::from([
            ("Node1".to_string(), PeerId::random()),
            ("Node2".to_string(), PeerId::random()),
            ("Node3".to_string(), PeerId::random())
        ]);

        let validators = vec![
            AngstromValidator::new(peers["Node1"], 100),
            AngstromValidator::new(peers["Node2"], 100),
            AngstromValidator::new(peers["Node3"], 100),
        ];

        let mut algo = WeightedRoundRobin::new(validators, BlockNumber::default());

        // Track selections over multiple rounds
        let mut selections = HashMap::new();
        for i in 1..=30 {
            let proposer = algo.choose_proposer(i).unwrap();
            *selections.entry(proposer).or_insert(0) += 1;
        }

        // With equal voting power, each validator should be selected roughly equally
        for count in selections.values() {
            assert!(*count >= 9 && *count <= 11, "Selection should be roughly equal");
        }
    }

    #[test]
    fn test_large_block_jump() {
        let (_, validators) = create_test_validators();
        let mut algo = WeightedRoundRobin::new(validators, 10);

        // Jump many blocks ahead
        let proposer = algo.choose_proposer(1000);
        assert!(proposer.is_some());
        assert_eq!(algo.block_number, 1000);
    }

    #[test]
    fn test_extreme_voting_power_differences() {
        let peers = HashMap::from([
            ("Whale".to_string(), PeerId::random()),
            ("Minnow".to_string(), PeerId::random())
        ]);

        let validators = vec![
            AngstromValidator::new(peers["Whale"], 1000000), // Very high voting power
            AngstromValidator::new(peers["Minnow"], 1),      // Minimal voting power
        ];

        let mut algo = WeightedRoundRobin::new(validators, BlockNumber::default());

        let mut whale_count = 0;
        let mut minnow_count = 0;

        // Run for 100 rounds
        for i in 1..=100 {
            let proposer = algo.choose_proposer(i).unwrap();
            if proposer == peers["Whale"] {
                whale_count += 1;
            } else {
                minnow_count += 1;
            }
        }

        // Whale should be selected significantly more often
        assert!(whale_count > 95, "High voting power validator should dominate selection");
        assert!(minnow_count < 5, "Low voting power validator should rarely be selected");
    }

    #[test]
    fn test_priority_uncenter_unscale() {
        let (_, validators) = create_test_validators();
        let mut algo = WeightedRoundRobin::new(validators, BlockNumber::default());

        // Set some initial priorities
        algo.validators = algo
            .validators
            .drain()
            .enumerate()
            .map(|(i, mut v)| {
                v.priority = i as i64 * 1000;
                v
            })
            .collect();

        let init = algo.clone();

        // Apply centering and scaling 10 times
        for _ in 0..10 {
            algo.scale_priorities();
            algo.center_priorities();
        }

        // unscale 10 times
        for _ in 0..10 {
            let avg = algo.unscale_priorities();
            algo.uncenter_priorities(avg);
        }

        for v in algo.validators {
            let start = init.validators.get(&v).unwrap();
            assert_eq!(&v, start)
        }
    }

    #[test]
    fn test_unscale_edge_cases() {
        let (_, validators) = create_test_validators();
        let mut algo = WeightedRoundRobin::new(validators, BlockNumber::default());

        // Test with zero priorities
        algo.validators = algo
            .validators
            .drain()
            .map(|mut v| {
                v.priority = 0;
                v
            })
            .collect();

        algo.unscale_priorities();
        assert!(
            algo.validators.iter().all(|v| v.priority == 0),
            "Zero priorities should remain zero after unscaling"
        );

        // Test with equal priorities
        algo.validators = algo
            .validators
            .drain()
            .map(|mut v| {
                v.priority = 100;
                v
            })
            .collect();

        algo.unscale_priorities();
        assert!(
            algo.validators.iter().all(|v| v.priority == 100),
            "Equal priorities should remain equal after unscaling"
        );
    }

    #[test]
    fn test_uncenter_edge_cases() {
        let (_, validators) = create_test_validators();
        let mut algo = WeightedRoundRobin::new(validators, BlockNumber::default());

        // Test with zero priorities
        algo.validators = algo
            .validators
            .drain()
            .map(|mut v| {
                v.priority = 0;
                v
            })
            .collect();

        algo.uncenter_priorities(100);
        assert!(
            algo.validators.iter().all(|v| v.priority == 100),
            "All priorities should be equal to target average after uncentering from zero"
        );

        // Test with negative target average
        algo.uncenter_priorities(-100);
        assert!(
            algo.validators.iter().all(|v| v.priority == 0),
            "Priorities should be restored to original after opposite uncentering"
        );
    }

    #[test]
    fn test_proposer_rollback_consistency() {
        let (peers, validators) = create_test_validators();
        let mut algo = WeightedRoundRobin::new(validators, 1);

        // Move forward and collect proposers
        let mut forward_proposers = Vec::new();
        for block in 1..=5 {
            let proposer = algo.choose_proposer(block).unwrap();
            forward_proposers.push(proposer);
        }

        // Roll back and verify we get the same proposers in reverse
        for block in (1..=4).rev() {
            let proposer = algo.choose_proposer(block).unwrap();
            assert_eq!(
                proposer,
                forward_proposers[block as usize - 1],
                "Rolling back to block {} should yield same proposer as forward pass",
                block
            );
        }
    }

    #[test]
    fn test_proposer_alternating_directions() {
        let (peers, validators) = create_test_validators();
        let mut algo = WeightedRoundRobin::new(validators, 5);

        // Forward 2, back 1, forward 3, back 2 pattern
        let transitions = vec![7, 6, 9, 7];
        let mut expected_at_7 = None;

        for block in transitions {
            let proposer = algo.choose_proposer(block);

            // Remember proposer at block 7 for consistency check
            if block == 7 {
                if expected_at_7.is_none() {
                    expected_at_7 = proposer;
                } else {
                    assert_eq!(
                        proposer, expected_at_7,
                        "Proposer at block 7 should be consistent across transitions"
                    );
                }
            }
        }
    }

    #[test]
    fn test_proposer_large_jumps() {
        let (peers, validators) = create_test_validators();
        let mut algo = WeightedRoundRobin::new(validators, 1000);

        // Test large forward jump
        let forward_proposer = algo.choose_proposer(5000).unwrap();

        // Test large backward jump
        let backward_proposer = algo.choose_proposer(2000).unwrap();

        // Jump forward to same block again
        let repeat_proposer = algo.choose_proposer(5000).unwrap();

        assert_eq!(
            forward_proposer, repeat_proposer,
            "Same block number should yield same proposer regardless of jump direction"
        );
    }

    #[test]
    fn test_proposer_sequence_stability() {
        let (peers, validators) = create_test_validators();
        let mut algo1 = WeightedRoundRobin::new(validators.clone(), 1);
        let mut algo2 = WeightedRoundRobin::new(validators, 1);

        // First sequence: 1->5->3->7
        for block in [1, 5, 3, 7] {
            algo1.choose_proposer(block);
        }

        // Second sequence: 1->3->5->7
        for block in [1, 3, 5, 7] {
            algo2.choose_proposer(block);
        }

        // Both should end up with the same proposer at block 7
        assert_eq!(
            algo1.choose_proposer(7),
            algo2.choose_proposer(7),
            "Different paths to same block should yield same proposer"
        );
    }

    #[test]
    fn test_proposer_boundary_transitions() {
        let (peers, validators) = create_test_validators();
        let mut algo = WeightedRoundRobin::new(validators, 1);

        // Test transitions around zero
        let proposer_at_1 = algo.choose_proposer(1).unwrap();
        let proposer_at_0 = algo.choose_proposer(0).unwrap();
        let proposer_at_1_again = algo.choose_proposer(1).unwrap();

        assert_eq!(
            proposer_at_1, proposer_at_1_again,
            "Proposer should be consistent after crossing block zero"
        );

        // Test transitions with max block number
        let high_block = BlockNumber::from(u64::MAX - 2);
        let proposer_high = algo.choose_proposer(high_block).unwrap();
        let proposer_lower = algo.choose_proposer(1000).unwrap();
        let proposer_high_again = algo.choose_proposer(high_block).unwrap();

        assert_eq!(
            proposer_high, proposer_high_again,
            "Proposer should be consistent after large block number transitions"
        );
    }

    #[test]
    fn test_scale_unscale_priorities() {
        // Create test validators with specific voting powers
        let validators = vec![
            AngstromValidator::new(PeerId::random(), 100),
            AngstromValidator::new(PeerId::random(), 200),
            AngstromValidator::new(PeerId::random(), 300),
        ];
        let mut algo = WeightedRoundRobin::new(validators, BlockNumber::default());
        let start = algo.clone();
        algo.scale_priorities();
        let _ = algo.unscale_priorities();

        for v in start.validators {
            let cur = algo.validators.get(&v).unwrap();
            assert_eq!(&v, cur);
        }
    }

    #[test]
    fn test_proposer_selection_inverse() {
        let (_, validators) = create_test_validators();
        let mut algo = WeightedRoundRobin::new(validators, BlockNumber::default());

        // Save initial state
        let initial_state: HashSet<AngstromValidator> = algo.validators.clone();

        // Apply proposer selection
        let proposer = algo.proposer_selection().unwrap();

        // Apply reverse selection
        let reverse_proposer = algo.reverse_proposer_selection().unwrap();

        // Check that we got back the same proposer
        assert_eq!(
            proposer, reverse_proposer,
            "Forward and reverse selections should return same proposer"
        );

        // Check that final state matches initial state
        assert_eq!(
            algo.validators, initial_state,
            "State should be restored after inverse operations"
        );
    }

    #[test]
    fn test_round_robin_simulation() {
        let peers = HashMap::from([
            ("Alice".to_string(), PeerId::random()),
            ("Bob".to_string(), PeerId::random()),
            ("Charlie".to_string(), PeerId::random())
        ]);
        let validators = vec![
            AngstromValidator::new(peers["Alice"], 100),
            AngstromValidator::new(peers["Bob"], 200),
            AngstromValidator::new(peers["Charlie"], 300),
        ];
        let mut algo = WeightedRoundRobin::new(validators, BlockNumber::default());

        fn simulate_rounds(algo: &mut WeightedRoundRobin, rounds: usize) -> HashMap<PeerId, usize> {
            let mut stats = HashMap::new();
            for i in 1..=rounds {
                let proposer = algo.choose_proposer(BlockNumber::from(i as u64)).unwrap();
                *stats.entry(proposer).or_insert(0) += 1;
            }
            stats
        }

        let rounds = 1000;
        let stats = simulate_rounds(&mut algo, rounds);

        assert_eq!(stats.len(), 3);

        let total_selections: usize = stats.values().sum();
        assert_eq!(total_selections, rounds);

        let alice_ratio = *stats.get(&peers["Alice"]).unwrap() as f64 / rounds as f64;
        let bob_ratio = *stats.get(&peers["Bob"]).unwrap() as f64 / rounds as f64;
        let charlie_ratio = *stats.get(&peers["Charlie"]).unwrap() as f64 / rounds as f64;

        assert!((alice_ratio - 0.167).abs() < 0.05);
        assert!((bob_ratio - 0.333).abs() < 0.05);
        assert!((charlie_ratio - 0.5).abs() < 0.05);
    }
}
