# Prototype Implementation of a Deterministic Peer-to-Peer Poker Engine in Rust Using Lock-Step Hash-Chain Replication
## Section 1: Motivation
TODO

## Section 2: Specifying the Game and deriving needed Abstractions.
### Texas Hold’em Rules: Flow of a Hand
At the beginning of the first hand of play, one player will be assigned the dealer button (in home games, this player will also traditionally act as the dealer for that hand). The player immediately to the left of the button must post the small blind, while the player two seats to the left of the button must post the big blind. The size of these blinds is typically determined by the rules of the game. If any ante is required – common in a tournament situation – players should also contribute it at this point.

Once all blinds have been posted and antes have been paid, the dealer will deal two cards to each player. Each player may examine their own cards. The play begins with the player to the left of the big blind. That player may choose to fold, in which case they forfeit their cards and are done with play for that hand. The player may also choose to call the bet, placing an amount of money into the pot equal to the size of the big blind. Finally, the player can also choose to raise, increasing the size of the bet required for other players to stay in the hand.

Moving around the table clockwise, each player may then choose to take any of those options: folding, calling the current bet, or raising the bet. A round of betting ends when all players but one have folded (in which case the one remaining player wins the pot), or when all remaining players have called the current bet. On the first round of betting, if no players raise, the big blind will also have the option to check, essentially passing his turn; this is because the big blind has already placed the current bet amount into the pot, but hasn’t yet had a chance to act.

Assuming there are two or more players remaining in the hand after the first round of betting, the dealer will then deal out three community cards in the middle of the table. These cards are known as the flop. Play now begins, starting with the first player to the left of the dealer button (if every player is still in the hand, this will be the small blind). Players have the same options as before; in addition, if no bet has yet been made in the betting round, players have the option to check. A round of betting can also end if all players check and no bets are made, along with the other ways discussed above.

If two or more players remain in the hand after the second round of betting, the dealer will place a fourth community card – known as the turn – on the table. Once again, a round of betting ensues, using the same rules outlined above. Finally, if two or more players are still around after the third round of betting, the dealer will place the final community card – the river – on the table. One last round of betting will commence.

After this final round of betting, all remaining players must reveal their hands. The player with the best hand according to the hand rankings above will win the pot. If two or more players share the exact same hand, the pot is split evenly between them. After each hand, the button moves one seat to the left, as do the responsibilities of posting the small and big blinds. [1]

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

##  Section 3: Architecture
## Section 3.1: General overview.
The architecture of the application can be abstracted into mulitple layers, where each layer builds upon the layer below, and each lower layer provides a service to the next upper layer.
The layers are as follows:
1. Presentation layer (GUI).
2. Computation layer: Specifies how the server computes state transitions, and how the client programs update their internal game states.
2. Communication Layer: Specifies the protocols which define how the clients communicate with the server and with each other to (1) reach consensus on the game state, and (2) exchange information with each other in general.
3. Networking layer: contains the specification on how clients (1) initialize a lobby, (2) identify each other, (3) address each other, (4) send messages to each other, (5) reconnect/disconnect etc.


### Section 3.2: Networking Layer
TODO:
1. how do clients and server initialize 'Session'
2. encryption of traffic.
3. public keys as identifiers
4. signing of messages via private key.

### Section 3.3 Communication Protocols
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
![state_transition_comunication_5](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_5.drawio.svg)

6. The Client acknowledges receiving the message. If the acknowledgement does not arrive at the server, he tries resends it in  exponentially increasing time intervals.\
![state_transition_comunication_6](https://github.com/random998/bachelors_project/blob/main/docs/state_transition_communication_6.drawio.svg)

7. Finally an error at the Client is raised, informing him about the failure of the application of his proposed state transition.

[1]: https://web.archive.org/web/20250517014634/https://www.texasholdemonline.com/texas-holdem-rules/

### Section 3.4: Computation Layer

### Section 3.5: Presentation Layer (GUI)
