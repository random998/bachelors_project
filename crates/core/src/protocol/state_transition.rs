use serde::{Deserialize, Serialize};
use crate::crypto::{PeerId, Signature};
use crate::protocol::msg::{Hash, Transition};
use crate::zk::Proof;

pub trait PrevHash {
    fn prev_hash(&self) -> Hash;
}
pub enum ProtocolMsg {
    StateTransitionProposal(StateTransitionProposal),
    STProposalAck(STProposalAck),
    STProposalNAck(STProposalNAck),
}

// Implement PrevHash for ProtocolMsg by matching on variants
impl PrevHash for ProtocolMsg {
    fn prev_hash(&self) -> Hash {
        match self {
            ProtocolMsg::StateTransitionProposal(proposal) => proposal.prev_hash(),
            ProtocolMsg::STProposalAck(ack) => ack.prev_hash(),
            ProtocolMsg::STProposalNAck(nack) => nack.prev_hash(),
        }
    }
}



#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct StateTransitionProposal {
    time_stamp_micros: u64,
    sequence_number: u64,
    
    prev_hash: Hash, /// reference to the previous ProtocolMsg.
    hash: Hash,  /// hash of the senders current state.
    signature: Signature,
    payload:   Payload,
    allowed_appenders: Vec<PeerId>,
    
    /// zkVM proof that `step(prev, payload) -> next`
    zk_proof:     Proof,
}

impl PrevHash for StateTransitionProposal {
    fn prev_hash(&self) -> Hash {
        self.prev_hash.clone()
    }
}

/// Short for StateTransitionProposalAcknowledgement
pub struct  STProposalAck {
    time_stamp_micros: u64,
    sequence_number: u64,
    prev_hash: Hash,
    signature: Signature,
}

impl PrevHash for STProposalAck {
    fn prev_hash(&self) -> Hash {
        self.prev_hash.clone()
    }
}

/// Short for StateTransitionProposalAcknowledgement
pub struct  STProposalNAck {
    time_stamp_micros: u64,
    sequence_number: u64,
    prev_hash: Hash,
    signature: Signature,
}
impl PrevHash for STProposalNAck {
    fn prev_hash(&self) -> Hash {
        self.prev_hash.clone()
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq,)]
pub enum Payload {
    Transition(Transition),
}

// Function to verify previous hash (example implementation)
pub fn verify_prev_hash(prev_state_transition: StateTransitionProposal, msg: ProtocolMsg) -> bool {
    let prev_hash = msg.prev_hash();
    let bytes = bincode::serialize(&prev_state_transition).unwrap();
    let hash = Hash(blake3::hash(&bytes));
    hash == prev_hash
}

