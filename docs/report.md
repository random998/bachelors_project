# Prototype Implementation of a Deterministic Peer-to-Peer Poker Engine in Rust Using Lock-Step Hash-Chain Replication
## Section 1: Motivation
TODO
##  Section 2: Architecture
The architecture of the application can be abstracted into mulitple layers, where each layer builds upon the layer below, and each lower layer provides a service to the next upper layer.
The layers are as follows:
1. Presentation layer (GUI).
2. Computation layer: Specifies how the server computes state transitions, and how the client programs update their internal game states.
2. Communication Layer: Specifies the protocols which define how the clients communicate with the server and with each other to (1) reach consensus on the game state, and (2) exchange information with each other in general.
3. Networking layer: contains the specification on how clients (1) initialize a lobby, (2) identify each other, (3) address each other, (4) send messages to each other, (5) reconnect/disconnect etc.

### Section 2.2: Computation layer
#### Definition of Terms:

#### Defining the game
The game which is implemented is the standard version of texas No-Limit holdem' as stated in the [wikipedia page](https://en.wikipedia.org/wiki/Texas_hold_%27em#Rules).
In no-limit hold 'em, players may bet or raise any amount over the minimum raise up to all of the chips the player has at the table (called an all-in bet).
The minimum raise is equal to the size of the previous bet or raise.
If someone wishes to re-raise, they must raise at least the amount of the previous raise. 
All terms used in this document refer to the terms as specified in this [glossary](https://en.wikipedia.org/wiki/Glossary_of_poker_terms).

Phases of the game:
1. ShufflingPhase
2. CardDealingPhase
3. PreflopBetting
4. Flop
5. FlopBetting
6. Turn
7. TurnBetting
8. River
9. RiverBetting
10. Showdown

ShufflingPhase:
A standard 52-card deck without jokers is shuffled by the server.

CardDealingPhase:
Each of the players is dealt 2 cards 'face-down' by the server. This is achieved by encrypting the cards in the following way: It is assumed that the server has access to the public keys of each player. The cards dealt to player1 are simply encrypted using player1's public key. Player1 can then decrypt his cards using his corresponding private key.

Betting Phases (PreflopBetting / FlopBetting / TurnBetting / RiverBettin):





#### Defining the (legal) state space.

#### Computation of State Transitions.
TODO

### Section 2.3 Communication Protocols
#### Communication between the Clients and the Server for a state Transition
Below, see an outline of the communication protocol between the clients and the server for transitioning the game state.\

1. If at any client an event occurs, which modifies the state of the game, the client sends a StateTransitionProposalMessage to the server.\
 ![state_transition_comunication_1](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_1.drawio.svg)

2. Upon receiving the message, and verifying the message's sender, the server sends an acknwoledge to the client which sent the message.\
 ![state_transition_comunication_2](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_2.drawio.svg)

3. Next, the server checks the proposed StateTransition for legality. If it is legal he transitions his internal GameState and informs all clients about the updated GameState.\
 ![state_transition_comunication_3](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_3.drawio.svg)

4. Upon receiving the StateUpdateMessage by the Server, the clients acknowledge it.\
![state_transition_comunication_4](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_4.drawio.svg)

If there is a deviation from the protocol specified above, either
(a) by a acknowledgement not arriving as specified.\
(b) by the StatetransitionProposal leading to an illegal GameState,

5. The server sends a StateTransitionFailedMessage to the Client who sent the initial proposal.\
![state_transition_comunication_4](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_5.drawio.svg)

6. The Client acknowledges receiving the message. If the acknowledgement does not arrive at the server, he tries resends it in  exponentially increasing time intervals.\
![state_transition_comunication_4](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_6.drawio.svg)

7. Finally an error at the Client is raised, informing him about the failure of the application of his proposed state transition.

