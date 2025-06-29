pub use self::types::*;
pub mod types;
mod vote_pool;
mod missing;

use p2p_net::{P2pTx, P2pRx};
use poker_core::message::{SignedMessage, Message};
use ahash::AHashSet;

pub struct TableEngine<Cb: EngineCallbacks> {
    callbacks: Cb,
    // config
    f: usize,
    threshold: usize,

    // runtime
    turn: Turn,
    hash_current: StateHash,
    votes: vote_pool::VotePool,
    missing: missing::AbsenceTracker,

    net_tx: P2pTx,
    net_rx: P2pRx,
}

/// Notifications that the host application must implement.
pub trait EngineCallbacks: Send + Sync + 'static {
    fn send(&self, player_id: PeerId, msg: SignedMessage);
    fn disconnect(&self, player_id: PeerId);
    fn credit_chips(&self, player_id: PeerId, amount: Chips);
}
