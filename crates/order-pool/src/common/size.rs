use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SizeTracker {
    pub max:     Option<usize>,
    pub current: usize
}

impl SizeTracker {
    #[allow(dead_code)]
    pub fn new(max: Option<usize>) -> Self {
        Self { max, current: 0 }
    }

    pub fn has_space(&mut self, size: usize) -> bool {
        if let Some(max) = self.max {
            if self.current + size <= max {
                self.current += size;
                true
            } else {
                false
            }
        } else {
            self.current += size;
            true
        }
    }

    #[allow(dead_code)]
    pub fn remove_order(&mut self, size: usize) {
        self.current -= size;
    }
}
