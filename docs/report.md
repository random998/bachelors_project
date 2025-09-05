# Prototype Implementation of a Deterministic Peer-to-Peer Poker Engine in Rust Using Lock-Step Hash-Chain Replication

### Communication between the Clients and the Server for a state Transition
Below, see an outline of the communication protocol between the clients and the server for transitioning the game state. \

1. If at any client an event occurs, which modifies the state of the game, the client sends a StateTransitionProposalMessage to the server. \
 ![state_transition_comunication_1](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_1.drawio.svg)

2. Upon receiving the message, and verifying the message's sender, the server sends an acknwoledge to the client which sent the message. \
 ![state_transition_comunication_2](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_2.drawio.svg)

3. Next, the server checks the proposed StateTransition for legality. If it is legal he transitions his internal GameState and informs all clients about the updated GameState. \
 ![state_transition_comunication_3](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_3.drawio.svg)

4. Upon receiving the StateUpdateMessage by the Server, the clients acknowledge it. \
![state_transition_comunication_4](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_4.drawio.svg)


If there is a deviation from the protocol specified above, either
(a) by a acknowledgement not arriving as specified.
(b) by the StatetransitionProposal leading to an illegal GameState,

5. The server sends a StateTransitionFailedMessage to the Client who sent the initial proposal.
![state_transition_comunication_4](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_5.drawio.svg)

6. The Client acknowledges receiving the message. If the acknowledgement does not arrive at the server, he tries resends it in  exponentially increasing time intervals.
![state_transition_comunication_4](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_6.drawio.svg)

7. Finally an error at the Client is raised, informing him about the failure of the application of his proposed state transition.
