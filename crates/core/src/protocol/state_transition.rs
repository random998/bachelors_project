use serde::{Deserialize, Serialize};
use crate::crypto::{PeerId, PublicKey, Signature};
use crate::protocol::msg::{Hash, Transition};
use crate::zk::Proof;

pub trait PrevHash {
    fn prev_hash(&self) -> Hash;
}

pub trait SeqNum {
    fn seq_num(&self) -> SequenceNumber;
}
pub enum ProtocolMsg {
    StateTransitionProposal(StateTransitionProposal),
    STProposalAck(STProposalAck),
}

// Implement PrevHash for ProtocolMsg by matching on variants
impl PrevHash for ProtocolMsg {
    fn prev_hash(&self) -> Hash {
        match self {
            ProtocolMsg::StateTransitionProposal(proposal) => proposal.prev_hash(),
            ProtocolMsg::STProposalAck(ack) => ack.prev_hash(),
        }
    }
}

impl SeqNum for ProtocolMsg {
    fn seq_num(&self) -> SequenceNumber {
        match self {
            ProtocolMsg::StateTransitionProposal(proposal) => proposal.get_seq_number(),
            ProtocolMsg::STProposalAck(ack) => ack.get_seq_number(),
        }
        
    }
}



#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct StateTransitionProposal {
    time_stamp_micros: TimeStamp,
    sequence_number: SequenceNumber,
    
    prev_hash: Hash, /// reference to the previous ProtocolMsg.
    sig: Signature,
    pubk: PublicKey,
    payload:   Payload,
    /// players that are allowed to act upon this message (adding a new StateTransitionProposal after this one).
    legal_actors: Vec<PeerId>,
    
    /// zkVM proof that `step(prev, payload) -> next`
    zk_proof:     Proof,
}

impl StateTransitionProposal {
    pub fn get_legal_actors(&self) -> Vec<PeerId> {
        self.legal_actors.clone()
    }
    
    pub fn get_seq_number(&self) -> SequenceNumber {
        self.sequence_number.clone()
    }
    
    pub fn get_payload(&self) -> Payload {
        self.payload.clone()
    }
    
    pub fn get_prev_hash(&self) -> Hash {
        self.prev_hash.clone()
    }
    
    pub fn sender(&self) -> PeerId {
        self.get_pubk().to_peer_id()
    }
    
    pub fn get_pubk(&self) -> PublicKey {
        self.pubk.clone()
    }
    
    pub fn get_sig(&self) -> Signature {
        self.sig.clone()
    }
}

impl PrevHash for StateTransitionProposal {
    fn prev_hash(&self) -> Hash {
        self.prev_hash.clone()
    }
}

/// Short for StateTransitionProposalAcknowledgement
pub struct  STProposalAck {
    time_stamp_micros: TimeStamp,
    sequence_number: SequenceNumber, 
    prev_hash: Hash,
    signature: Signature,
    public_key: PublicKey,
}

impl STProposalAck {
    pub fn get_seq_number(&self) -> SequenceNumber {
        self.sequence_number.clone()
    }
    pub fn sender(&self) -> PeerId {
        self.public_key.to_peer_id()
    }
    
    pub fn is_valid(&self) -> bool {
        //todo
        true
    }
}


#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash)]
pub struct SequenceNumber(u64);
#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Hash)]
pub struct TimeStamp(u64);

impl PrevHash for STProposalAck {
    fn prev_hash(&self) -> Hash {
        self.prev_hash.clone()
    }
}

/// Short for StateTransitionProposalAcknowledgement
pub struct  STProposalNAck {
    time_stamp_micros: TimeStamp,
    sequence_number: SequenceNumber,
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

impl Payload {
    pub fn is_legal_transition(&self) -> bool {
        match self {
            Payload::Transition(transition) => {
                transition.is_legal()
            }
        } 
    }
}

// Function to verify previous hash (example implementation)
pub fn verify_prev_hash(prev_state_transition: StateTransitionProposal, msg: ProtocolMsg) -> bool {
    let prev_hash = msg.prev_hash();
    let bytes = bincode::serialize(&prev_state_transition).unwrap();
    let hash = Hash(blake3::hash(&bytes));
    hash == prev_hash
}
