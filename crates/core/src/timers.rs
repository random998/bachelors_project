// crates/core/src/timers.rs
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::crypto::PeerId;

#[derive(Debug, Default,)]
pub struct Timers {
    /// action deadline for every player that is currently “on turn”
    /// (inserted when the `ActionRequest` is issued; removed after action)
    deadlines: HashMap<PeerId, Instant,>,
}

impl Timers {
    pub fn set(&mut self, pid: PeerId, after: Duration,) {
        self.deadlines.insert(pid, Instant::now() + after,);
    }

    pub fn clear(&mut self, pid: &PeerId,) {
        self.deadlines.remove(pid,);
    }

    /// Returns the list of players whose deadline has expired *right now*
    #[must_use]
    pub fn expired(&self,) -> Vec<PeerId,> {
        let now = Instant::now();
        self.deadlines
            .iter()
            .filter_map(|(pid, &t,)| {
                if t <= now {
                    Some(*pid,)
                } else {
                    None
                }
            },)
            .collect()
    }
}
