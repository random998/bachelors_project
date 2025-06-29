use super::*;

#[derive(Default)]
pub struct AbsenceTracker {
    streak: hashbrown::HashMap<PeerId, u8>,
    limit:  u8,        // e.g. 2
}

impl AbsenceTracker {
    pub fn new(limit: u8) -> Self { Self { limit, ..Default::default() } }

    /// call after a commit, feeding the quorum set
    pub fn update(&mut self, quorum: &hashbrown::HashSet<PeerId>) -> Vec<PeerId> {
        let mut kicked = Vec::new();
        for (&pid, s) in self.streak.iter_mut() {
            if quorum.contains(&pid) { *s = 0 } else { *s += 1 }
            if *s >= self.limit { kicked.push(pid) }
        }
        // initialise new peers
        for pid in quorum {
            self.streak.entry(*pid).or_default();
        }
        kicked
    }
}

