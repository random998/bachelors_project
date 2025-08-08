use crate::crypto::PeerId;
use crate::protocol::msg::Hash;
use crate::protocol::state_transition::{PrevHash, ProtocolMsg, SeqNum, SequenceNumber, StateTransitionProposal};

pub struct State {
    /// history of all state transitions applied to the state of this peer ordered from oldest to latest. 
    state_transitions: Vec<StateTransitionProposal>,
    /// hash of the last state transition applied to the state of this local peer_id.
    prev_hash: Hash,
    /// most recent (legitimate) proposed state transition sent by this peer or received from other peers.
    state_transition_proposal: Option<StateTransitionProposal>,
    /// tracks which peers have acknowledged state_transition_proposal.
    acknowledgements: std::collections::HashMap<SequenceNumber, Vec<PeerId>>,
    /// blacklist of peer_ids: all messages from those peers are to be discarded.
    blacklist: Vec<PeerId>,
    /// all peers currently participating in the game. 
    peers: Vec<PeerId>,
    /// seen sequence numbers.
    seq_numbers: Vec<SequenceNumber>,
    /// our own peer id.
    peer_id: PeerId,
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
        let seq_num = proposal.get_seq_number();
        if let Some(acks) = self.acknowledgements.get(&seq_num) {

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
    
    pub fn has_unique_seq(&self, protocol_msg: ProtocolMsg) -> bool {
        !self.seq_numbers.contains(&protocol_msg.seq_num())
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
            
            // check if the peerId is the lowest of the allowed appenders, only then is he allowed to append. 
            for actor in legal_actors {
                if actor < proposal_sender {
                    return false;
                }
            }
            true
        }
    }
    
    pub fn is_valid(&self, proposal: &StateTransitionProposal) -> bool {
        if !self.is_legal_appender(proposal) {
            return false;
        }
        if self.is_blacklisted(&proposal.sender()) {
            return false;
        };
        if !proposal.get_payload().is_legal_transition() {
            return false;
        }
        if !self.has_valid_prev_hash(proposal) {
            return false;
        }
        true
    }
    
    pub fn has_valid_prev_hash(&self, proposal: &StateTransitionProposal) -> bool {
        let prev_hash = proposal.get_prev_hash();
        prev_hash == self.prev_hash
    }
    
    pub fn apply(&mut self, msg: &ProtocolMsg) {
        match msg {
            ProtocolMsg::StateTransitionProposal(proposal) => {
                // if there already exists proposal, waiting to be acknowledged, discard.
                if self.state_transition_proposal != None {
                    return;
                }
                // do not accept invalid proposals.
                if !self.is_valid(proposal) {
                    return;
                }
                
                // otherwise set to current state_transition_proposal and add sender to acknowledger's list.
                self.state_transition_proposal = Some(proposal.clone()); 
                let mut acks = self.acknowledgements.get(&proposal.get_seq_number()).unwrap().clone();
                acks.push(proposal.sender());
                // also add ourselves to the list of acknowledgers since we checked if the proposal is valid.
                acks.push(self.peer_id);
                self.acknowledgements.insert(proposal.get_seq_number(), acks);
                
                
            }
            ProtocolMsg::STProposalAck(ack) => {
               // check if the message is valid.
                if !ack.is_valid() {
                    return;
                } 
                    
                // check if the referenced proposal is the one which is currently stored in our buffer.
                let mut acks = Vec::new();
                match self.state_transition_proposal.clone() {
                    None => {
                        return;
                    }
                    Some(proposal) => {
                        if proposal.get_seq_number() != ack.get_seq_number() {
                            return;
                        }
                        if let Some(acks) = self.acknowledgements.get(&ack.get_seq_number()) {
                            let mut acks = acks.clone();
                            acks.push(proposal.sender());
                        } else {
                            acks.push(proposal.sender());
                        }
                        self.acknowledgements.insert(ack.get_seq_number(), acks);
                    }
                }
            }
            _ => {}
        };
        self.seq_numbers.push(msg.seq_num());
    }
}
