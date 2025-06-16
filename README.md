Bachelors project Scope: Development of a p2p zero-knowledge poker game in Rust.

Roadmap:

1. web client:
    1.1 first starter screen application using egui.
    1.2 webclient structure / screen organization.
    1.3 window/log/console that logs every action that happens & every interaction & message with any other client.    

2. poker hand evaluator.
3. p2p lobby creation / message exchange / reconnects / disconnects.
4. account/ID management.
5. Betting logic.
6. p2p game state synchronization (blockchain?):
    requirements:
    6.1 No player acts as a permanent server.
    6.2 Any player can host a lobby, but the game state is replicated.
    6.3 Game must continue if any peer (including host) disconnects.
    6.5 All messages must be authenticated and verifiable (Noise + signature).
    6.6 Consensus on game state must be maintained among peers.
7. message passing protocol.

Bachelors thesis: Scope: Improved Efficient ZK Poker protocol + implementation
1. Improved secure shuffling protocol.
2. publishing of commitments to game participants.
3. Verificatoin of commitments/proofs by other game participants.
4. Penalties to Players who do not follow the agreed upon protocol.
5. Metamask crypto wallet connections?
6. Improved shuffle protocol.
7. Choice of ZK circuit development stack.
8.  Implementation of ZK circuits.
9.  Proof of correctness for the protocol.