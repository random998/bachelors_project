Bachelors project Scope: Development of a p2p zero-knowledge poker game in Rust.

Roadmap:

1. p2p game state synchronization (blockchain?):
    requirements:
    6.1 No player acts as a permanent server.
    6.2 Any player can host a lobby, but the game state is replicated.
    6.3 Game must continue if any peer (including host) disconnects.
    6.5 All messages must be authenticated and verifiable (Noise + signature).
    6.6 Consensus on game state must be maintained among peers.
7. message passing protocol (DONE? clients just broadcast messages.)
8. Lobby Advertisement (Peer Discovery)
    8.1 Use multicast, DNS-SD, libp2p, or DHT to allow a player to advertise a lobby.
    8.2 Peers can discover open games and connect via Noise-encrypted WebSockets.
    8.3 No peer should be a permanent coordinator.
9. Decentralized Game State
    9.1 Replicate the full game state (table, pot, players, deck hash, etc.) across all peers.
    9.2 Each peer runs a local state machine.
    9.3 Use signed, timestamped messages and broadcast every state transition.
10. Consensus Protocol?
    10.1 Use a deterministic protocol to decide turn order and resolve actions:
    E.g., fixed round-robin for turns.
    10.2 Each action includes a signed ActionMessage.
    10.3 Optionally use a simple majority or BFT protocol for validating transitions.
    10.4 Implement a rollback/timeout system for stalled or invalid actions.

11. look at C open source web poker implementation and compare with my own! Incorporate parts of the code if feasible.
12. look at source code of dark forest zk game, note how they "did things", especially how they implemented the debugging console.
13. ui improvements:
    display the current game phase somwehere
    display the id's of the players at all times.
14. Add code smell analysis and tools like in sopra.
15. Add more extensive tests.


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
10. Deck Shuffling and Card Dealing (Fairness)
    Implement verifiable shuffling (e.g., Mental Poker):
    Each player encrypts and permutes the deck using their public key.
    Cards are revealed through cooperative decryption.
    Prevents cheating and allows verifiable fairness.
    Final thesis polishing: references, proofreading, formatting
    Create README, reproducibility guide, and demo materials

September 23:

    Submit the final thesis

