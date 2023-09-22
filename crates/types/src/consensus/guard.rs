use std::{collections::HashSet, hash::Hash};

use reth_primitives::H512;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardSet {
    pub guards:             HashSet<GuardInfo>,
    pub leader:             Option<GuardInfo>,
    pub total_voting_power: u64
}

impl GuardSet {
    pub fn contains_key(&self, key: &H512) -> bool {
        self.guards.contains(key)
    }

    pub fn len(&self) -> usize {
        self.guards.len()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardInfo {
    pub pub_key:         H512,
    pub voting_power:    u64,
    pub leader_priority: u64
}

impl PartialEq for GuardInfo {
    fn eq(&self, other: &Self) -> bool {
        self.pub_key == other.pub_key
    }
}
impl Eq for GuardInfo {}

impl Hash for GuardInfo {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.pub_key.hash(state);
    }
}
