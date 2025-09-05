# Prototype Implementation of a Deterministic Peer-to-Peer Poker Engine in Rust Using Lock-Step Hash-Chain Replication

### Outline of the communication between the Clients and the Server for a state Transition

If at any client an event occurs, which modifies the state of the game, the client sends a StateTransitionProposalMessage to the server, containing
(1) a representation of the current GameState at the client, and (2) a specification of the event soliciting him to send the message

![State Transition communication](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication.svg)

If there is a deviation from the protocol specified above, either by a acknowledgement not arriving as specified, or the Statetransition of the server being faulty, the server sends a TransitionFailed Message to the peer initiliazing the transition, notifying him of a failure of the proposed state transition.
