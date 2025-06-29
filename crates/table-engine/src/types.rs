use poker_core::crypto::PeerId;
use std::num::NonZeroU32;

pub type Turn     = u64;
pub type StateHash = [u8; 32];              // blake2s output

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Vote {
    pub turn: Turn,
    pub hash: StateHash,
    pub signer: PeerId,
    pub sig: [u8; 64],     // raw ed25519 sig from SigningKey
}
