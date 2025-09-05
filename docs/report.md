# Prototype Implementation of a Deterministic Peer-to-Peer Poker Engine in Rust Using Lock-Step Hash-Chain Replication

### Communication between the Clients and the Server for a state Transition
Below, the communication protocol between the clients and the server for transition the game state is outlined. \

1. If at any client an event occurs, which modifies the state of the game, the client sends a StateTransitionProposalMessage to the server. \
 ![state_transition_comunication_1](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_1.drawio.svg)

2. Upon receiving the message, and verifying the message's sender, the server sends an acknwoledge to the client which sent the message. \
 ![state_transition_comunication_2](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_2.drawio.svg)

3. Next, the server checks the proposed StateTransition for legality. If it is legal he transitions his internal GameState and informs all clients about the updated GameState. \
 ![state_transition_comunication_3](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_3.drawio.svg)

4. Upon receiving the StateUpdateMessage by the Server, the clients acknowledge it. \
![state_transition_comunication_4](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_4.drawio.svg)


If there is a deviation from the protocol specified above, either
1. by a acknowledgement not arriving as specified (TODO Timeout specification), or 
2. by the StatetransitionProposal leading to an illegal GameState, or
3. by TODO

The server sends a StateTransitionFailedMessage to the Client who sent the initial proposal. 
