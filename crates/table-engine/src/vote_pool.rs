use super::*;
use hashbrown::{HashMap, HashSet};

#[derive(Default)]
pub struct VotePool {
    // turn  →  hash  → set(peer)
    inner: HashMap<Turn, HashMap<StateHash, HashSet<PeerId>>>,
}

impl VotePool {
    pub fn insert(&mut self, v: Vote, threshold: usize) -> Option<StateHash> {
        let m = self.inner
            .entry(v.turn)
            .or_default()
            .entry(v.hash)
            .or_default();
        m.insert(v.signer);

        if m.len() >= threshold { Some(v.hash) } else { None }
    }

    pub fn clear_turn(&mut self, t: Turn) { self.inner.remove(&t); }
}
