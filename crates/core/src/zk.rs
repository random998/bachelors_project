// poker_core/src/zk.rs
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, Default,)]
pub struct Proof(Vec<u8,>,);
#[derive(Clone, Debug, Serialize, Deserialize, Default,)]
pub struct Commitment(Vec<u8,>,);
pub type ShuffleProof = Proof;
pub type RangeProof = Proof;
