//! Helper container that keeps all per‑player records in order and offers
//! convenience helpers for turn‑management.  No locking is required because
//! the whole `InternalTableState` runs on a single Tokio task.

use rand::prelude::SliceRandom;
use rand::Rng;
use std::fmt;
use serde::{Deserialize, Serialize};
use crate::crypto::PeerId;
use crate::game_state::PlayerPrivate;

/// Thin wrapper around `Vec<PlayerPrivate>` with a few helpers that the
/// engine uses.  All indices are **stable** once a player has joined; we never
/// shuffle the vector directly – instead we keep a `button_idx` and
/// `active_idx` so we can rotate responsibility cheaply.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlayerStateObjects {
    players:    Vec<PlayerPrivate,>,
    button_idx: Option<usize,>, // who has the dealer button right now
    active_idx: Option<usize,>, // whose turn is it to act?
}

impl PlayerStateObjects {
    pub fn shuffle_seats<R: Rng>(&mut self, rng: &mut R) {
        self.players.shuffle(rng);
    }
    
    pub fn default() -> PlayerStateObjects {
        PlayerStateObjects {
            players: Vec::default(),
            button_idx: None,
            active_idx: None,
        }
    }
}

impl PlayerStateObjects {
    // ────────────────────────────────────────────────────────────────
    //  CRUD helpers
    // ────────────────────────────────────────────────────────────────

    pub fn add(&mut self, p: PlayerPrivate,) {
        self.players.push(p,);
    }

    pub fn get(&self, id: &PeerId,) -> Option<&PlayerPrivate,> {
        self.players.iter().find(|p| &p.id == id,)
    }

    pub fn get_mut(&mut self, id: &PeerId,) -> Option<&mut PlayerPrivate,> {
        self.players.iter_mut().find(|p| &p.id == id,)
    }

    pub fn remove(&mut self, id: &PeerId,) -> Option<PlayerPrivate,> {
        if let Some(pos,) = self.players.iter().position(|p| &p.id == id,) {
            // keep indices consistent
            if let Some(b,) = self.button_idx.filter(|&b| b >= pos,) {
                self.button_idx = Some(b.saturating_sub(1,),);
            }
            if let Some(a,) = self.active_idx.filter(|&a| a >= pos,) {
                self.active_idx = Some(a.saturating_sub(1,),);
            }
            return Some(self.players.remove(pos,),);
        }
        None
    }

    pub fn iter(&self,) -> impl Iterator<Item = &PlayerPrivate,> {
        self.players.iter()
    }
    pub fn iter_mut(&mut self,) -> impl Iterator<Item = &mut PlayerPrivate,> {
        self.players.iter_mut()
    }

    // ────────────────────────────────────────────────────────────────
    //  Queries used by the engine
    // ────────────────────────────────────────────────────────────────

    pub const fn count(&self,) -> usize {
        self.players.len()
    }
    pub fn count_active(&self,) -> usize {
        self.players.iter().filter(|p| p.is_active,).count()
    }
    pub fn count_with_chips(&self,) -> usize {
        self.players.iter().filter(|p| p.has_chips(),).count()
    }
    pub fn count_active_with_chips(&self,) -> usize {
        self.players
            .iter()
            .filter(|p| p.is_active && p.has_chips(),)
            .count()
    }

    /// Is the given player the one whose turn it is _right now_?
    pub fn is_active(&self, id: &PeerId,) -> bool {
        self.active_idx
            .and_then(|idx| self.players.get(idx,),)
            .is_some_and(|p| &p.id == id,)
    }

    /// Mutable reference to the player whose turn it is, if any.
    pub fn active_player_mut(&mut self,) -> Option<&mut PlayerPrivate,> {
        self.active_idx
            .and_then(move |i| self.players.get_mut(i,),)
            .filter(|p| p.is_active,)
    }
    
    pub fn active_player(&mut self) -> Option<&PlayerPrivate> {
        self.active_idx
            .and_then(move |i| self.players.get(i,),)
            .filter(|p| p.is_active,)
    }

    // ────────────────────────────────────────────────────────────────
    //  Turn / round helpers
    // ────────────────────────────────────────────────────────────────

    /// Called at the beginning of a **hand** – resets bets, rotates the
    /// dealer button, and chooses the first active player.
    pub fn start_hand<R: Rng,>(&mut self, rng: &mut R,) {
        // reset all players to default per‑hand state
        for p in &mut self.players {
            p.reset_for_new_hand();
        }

        if self.count_active() < 2 {
            self.active_idx = None;
            return;
        }

        // move dealer button one step clockwise; if there is none yet, pick a
        // random active player so first hand is fair.
        self.button_idx = match self.button_idx {
            Some(idx,) => Some((idx + 1) % self.players.len(),),
            None => {
                self.players.iter().position(|p| p.is_active,).or_else(|| {
                    // fallback: random active player
                    let choices: Vec<_,> = self
                        .players
                        .iter()
                        .enumerate()
                        .filter(|(_, p,)| p.is_active,)
                        .collect();
                    let idx = rng.random_range(0..choices.len(),);
                    choices.get(idx,).map(|(i, _,)| *i,)
                },)
            },
        };

        // first to act is the player left of the big blind (simplified)
        self.active_idx = self
            .players
            .iter()
            .cycle()
            .skip(self.button_idx.unwrap() + 1,)
            .position(|p| p.is_active && p.has_chips(),);
    }

    /// Move `active_idx` to the next seat that is both *active* **and** has
    /// chips. Does nothing if less than two such players remain.
    pub fn advance_turn(&mut self,) {
        if self.count_active_with_chips() <= 1 {
            return;
        }
        if let Some(cur,) = self.active_idx {
            let len = self.players.len();
            for i in 1..=len {
                // at most len steps
                let next = (cur + i) % len;
                let p = &self.players[next];
                if p.is_active && p.has_chips() {
                    self.active_idx = Some(next,);
                    break;
                }
            }
        }
    }

    /// Called when the hand ends. Resets indices and per‑player end‑of‑hand
    /// bookkeeping.
    pub fn end_hand(&mut self,) {
        self.active_idx = None;
        for p in &mut self.players {
            p.finalize_hand();
        }
    }

    /// Remove players who are out of chips.
    pub fn remove_bankrupt(&mut self,) {
        self.players
            .retain(PlayerPrivate::has_chips,);
        // indices might now be out‑of‑bounds; recompute safely
        self.button_idx = self.button_idx.filter(|&i| i < self.players.len(),);
        self.active_idx = self.active_idx.filter(|&i| i < self.players.len(),);
    }
}

impl fmt::Display for PlayerStateObjects {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "{:?}", self.players)
    }
}
