use alloy_primitives::Address;

const ONE_E3: u64 = 1000;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AngstromValidator {
    pub peer_id:  Address,
    voting_power: u64,
    priority:     i64
}

impl AngstromValidator {
    pub fn new(name: Address, voting_power: u64) -> Self {
        AngstromValidator {
            peer_id:      name,
            voting_power: voting_power * ONE_E3,
            priority:     0
        }
    }

    pub fn voting_power(&self) -> u64 {
        self.voting_power
    }

    pub fn priority(&self) -> i64 {
        self.priority
    }

    pub fn set_priority(&mut self, new_priority: i64) {
        self.priority = new_priority;
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
