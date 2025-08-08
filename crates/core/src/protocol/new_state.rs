use crate::crypto::PeerId;
use crate::protocol::msg::Hash;
use crate::protocol::state_transition::{PrevHash, ProtocolMsg, StateTransitionProposal};

pub struct State {
    /// history of all state transitions applied to the state of this peer ordered from oldest to latest. 
    state_transitions: Vec<StateTransitionProposal>,
    /// hash of the last state transition applied to the state of this local peer_id.
    prev_hash: Hash,
    /// most recent (legitimate) proposed state transition sent by this peer or received from other peers.
    state_transition_proposal: StateTransitionProposal,
    /// tracks which peers have acknowledged state_transition_proposal.
    acknowledgements: std::collections::HashMap<Hash, Vec<PeerId>>,
    /// blacklist of peer_ids: all messages from those peers are to be discarded.
    blacklist: Vec<PeerId>,
    /// all peers currently participating in the game. 
    peers: Vec<PeerId>,
}

impl State {
    /// verifies whether a ProtocolMessages prev_hash field contains a valid hash of the last applied state_transition_proposal.
    pub fn verify_prev_hash(prev_hash: Hash, msg: ProtocolMsg) -> bool {
        let msg_prev_hash =  msg.prev_hash();
        prev_hash == msg_prev_hash
    }
    
    /// returns a hash of our current state.
    pub fn hash(&self) -> Hash {
        todo!()
    }
    
    /// checks whether a peer is blacklisted.
    pub fn is_blacklisted(&self, peer_id: &PeerId) -> bool {
        self.blacklist.contains(peer_id)
    }

    /// Checks if consensus on a state_transition_proposal has been reached.
    /// Consensus requires all non-blacklisted peers in self.peers to have acknowledged.
    pub fn has_consensus(&self, proposal: &StateTransitionProposal) -> bool {
        // Get the list of peers who acknowledged this proposal
        let mut bytes = bincode::serialize(&proposal).unwrap();
        let hash = Hash(blake3::hash(bytes.as_mut_slice()));
        if let Some(acks) = self.acknowledgements.get(&hash) {

            // Filter out blacklisted peers from self.peers
            let active_peers : Vec<PeerId> = self.peers.iter().filter(|peer_id| !self.is_blacklisted(peer_id)).cloned().collect();

            for peer in active_peers {
                if !acks.contains(&peer) {
                    return false;
                }
            }
            true

        } else {
            // No acknowledgements for this proposal
            false
        }
    }
    
    pub fn is_legal_appender(&self, proposal: &StateTransitionProposal) -> bool {
        // check if the sender is contained in the allowed appenders of the most recent state transition.
        if self.state_transitions.len()==  0 {
            true
        } else {
            let legal_actors = self.state_transitions.last().unwrap().get_legal_actors();
            let proposal_sender = proposal.get_pubk().to_peer_id();
            if !legal_actors.contains(&proposal_sender) {
                return false;
            }
            true
        } 
    }
    
}
