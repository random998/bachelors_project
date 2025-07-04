Bachelors project Scope: Development of a p2p zero-knowledge poker game in Rust.

Roadmap:
- merge client game state and game state in engine.rs to one game state per peer.
- look at poker-rs crate for inspiration.
- check via tests whether game-state locig and game-state transition logic is implemented correctly.
- incorporate debug console like in zk-darkforest game
- ui improvements:
    display the current game phase somwehere
    display the id's of the players at all times.
    change keybindings of controls.
    change table ui slightly.
- code smell analysis and tools like in sopra.
- more extensive tests.
- email to examination office in order to register project & thesis.
- project report & submission


Bachelors thesis: Scope: Improved Efficient ZK Poker protocol + implementation
- research on how to do a distributed game state engine with conflict resolution.
- implement conflict resolution regarding game state
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