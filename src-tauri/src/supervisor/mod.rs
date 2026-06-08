pub mod process;
pub mod health;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildHandle {
    pub integration_id: String,
    pub pid: u32,
}

pub struct Supervisor {
    handles: HashMap<String, ChildHandle>,
}

impl Supervisor {
    pub fn new() -> Self {
        Self {
            handles: HashMap::new(),
        }
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}
