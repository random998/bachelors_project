Bachelors project Scope: Development of a p2p zero-knowledge poker game in Rust.

Roadmap:
- fix ui / game bugs.
- check via tests whether game-state logic and game-state transition logic is implemented correctly.
- email to examination office in order to register project & thesis.
- project report & (preliminary?) submission


Bachelors thesis: Scope: Improved Efficient ZK Poker protocol + implementation
- code smell analysis and tools like in sopra.
- incorporate debug console like in zk-darkforest game
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