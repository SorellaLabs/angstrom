use std::collections::HashSet;

use angstrom_types::consensus::Evidence;
use thiserror::Error;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum EvidenceCollectorError {
    #[error("Evidence collector already has this evidence")]
    AlreadyContainsEvidence,
    #[error("Could not find requested evidence")]
    NoEvidenceFound
}
#[allow(dead_code)]
pub struct EvidenceCollector {
    pending_evidence: HashSet<Evidence>
}

impl EvidenceCollector {
    #[allow(dead_code)]
    pub fn add_evidence(&mut self, evidence: Evidence) -> Result<(), EvidenceCollectorError> {
        if !self.pending_evidence.insert(evidence) {
            return Err(EvidenceCollectorError::AlreadyContainsEvidence)
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn check_evidence(&mut self, evidence: &Evidence) -> Result<bool, EvidenceCollectorError> {
        Ok(self.pending_evidence.contains(evidence)).and_then(|has| {
            if !has {
                Err(EvidenceCollectorError::NoEvidenceFound)
            } else {
                Ok(has)
            }
        })
    }
}
