//! Helper container that keeps all per‑player records in order and offers
//! convenience helpers for turn‑management.  No locking is required because
//! the whole `InternalTableState` runs on a single Tokio task.

use std::fmt;
use std::option::Option;

use rand::Rng;
use rand::prelude::SliceRandom;
use serde::{Deserialize, Serialize};

use crate::crypto::PeerId;
use crate::game_state::PlayerPrivate;
use crate::poker::Chips;

/// Thin wrapper around `Vec<PlayerPrivate>` with a few helpers that the
/// engine uses.  All indices are **stable** once a player has joined; we never
/// shuffle the vector directly – instead we keep a `button_idx` and
/// `active_idx` so we can rotate responsibility cheaply.
#[derive(Clone, Debug, Default, Serialize, Deserialize,)]
pub struct PlayerStateObjects {
    players:    Vec<PlayerPrivate,>,
    active_idx: Option<usize,>, // whose turn is it to act?
}

impl PlayerStateObjects {
    pub fn swap(&mut self, a: usize, b: usize,) {
        self.players.swap(a, b,);
    }

    pub fn rotate_left(&mut self, mid: usize,) {
        self.players.rotate_left(mid,);
    }
    pub fn shuffle_seats<R: Rng,>(&mut self, rng: &mut R,) {
        self.players.shuffle(rng,);
    }

    pub fn default() -> Self {
        Self {
            players:    Vec::default(),
            active_idx: None,
        }
    }

    pub fn players(&self,) -> Vec<PlayerPrivate,> {
        self.players.clone()
    }

    pub fn clear(&mut self,) {
        self.players.clear();
    }
}

impl PlayerStateObjects {
    // ────────────────────────────────────────────────────────────────
    //  CRUD helpers
    // ────────────────────────────────────────────────────────────────

    pub fn add(&mut self, p: PlayerPrivate,) {
        self.players.push(p,);
    }

    pub fn get(&mut self, id: &PeerId,) -> Option<&mut PlayerPrivate,> {
        self.players.iter_mut().find(|p| p.peer_id == *id,)
    }

    pub fn get_mut(&mut self, id: &PeerId,) -> Option<&mut PlayerPrivate,> {
        self.players.iter_mut().find(|p| &p.peer_id == id,)
    }

    pub fn remove(&mut self, id: &PeerId,) -> Option<PlayerPrivate,> {
        if let Some(pos,) = self.players.iter().position(|p| &p.peer_id == id,)
        {
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

    /// Activate the next player if there is more than one active player.
    pub fn activate_next_player(&mut self,) {
        if self.count_active() > 0 && self.active_player().is_some() {
            let active_player = self.active_idx.take().unwrap();

            // Iterate cyclically starting from the player after the active one.
            let iter = self
                .players
                .iter()
                .enumerate()
                .cycle()
                .skip(active_player + 1,)
                .take(self.players.len() - 1,);

            for (pos, p,) in iter {
                if p.is_active && p.chips > Chips::ZERO {
                    self.active_idx = Some(pos,);
                    break;
                }
            }
        }
    }

    /// Is the given player the one whose turn it is _right now_?
    pub fn is_active(&self, id: &PeerId,) -> bool {
        self.active_idx
            .and_then(|idx| self.players.get(idx,),)
            .is_some_and(|p| &p.peer_id == id,)
    }

    /// Mutable reference to the player whose turn it is, if any.
    pub fn active_player_mut(&mut self,) -> Option<&mut PlayerPrivate,> {
        self.active_idx
            .and_then(move |i| self.players.get_mut(i,),)
            .filter(|p| p.is_active,)
    }

    pub fn active_player(&mut self,) -> Option<&PlayerPrivate,> {
        self.active_idx
            .and_then(move |i| self.players.get(i,),)
            .filter(|p| p.is_active,)
    }

    // ────────────────────────────────────────────────────────────────
    //  Turn / round helpers
    // ────────────────────────────────────────────────────────────────
    /// Set state for a new hand.
    pub fn start_hand(&mut self,) {
        for player in &mut self.players {
            player.start_hand();
        }

        if self.count_active() > 1 {
            // Rotate players so that the first player becomes the button.
            loop {
                self.players.rotate_left(1,);
                if self.players[0].is_active {
                    // Checked above there are at least 2 active players, go
                    // back and set the button.
                    for p in self.players.iter_mut().rev() {
                        if p.is_active {
                            p.has_button = true;
                            break;
                        }
                    }

                    break;
                }
            }

            self.active_idx = Some(0,);
        } else {
            self.active_idx = None;
        }
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
        self.players.retain(PlayerPrivate::has_chips,);
        self.active_idx = self.active_idx.filter(|&i| i < self.players.len(),);
    }

    /// Starts a new round.
    pub fn start_round(&mut self,) {
        self.active_idx = None;

        // Set an active player at the beginning of a round only if there are
        // two or more player with chips.
        if self.count_active_with_chips() > 1 {
            for (idx, p,) in self.players.iter().enumerate() {
                if p.chips > Chips::ZERO && p.is_active {
                    self.active_idx = Some(idx,);
                    return;
                }
            }
        }
    }

    /// Remove players that run out of chips.
    pub fn remove_with_no_chips(&mut self,) {
        self.players.retain(|p| p.chips > Chips::ZERO,);
    }
}

impl fmt::Display for PlayerStateObjects {
    fn fmt(&self, f: &mut fmt::Formatter<'_,>,) -> fmt::Result {
        write!(f, "{:?}", self.players)
    }
}
