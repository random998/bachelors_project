Bachelors project Scope: Development of a p2p zero-knowledge poker game in Rust.

Roadmap:
- clean up log statements to get a better grasp on what is going the fuck on.
- look at C open source web poker implementation and compare with my own! Incorporate parts of the code if feasible.
- look at source code of dark forest zk game, note how they "did things", especially how they implemented the zk commi
display console.
- ui improvements:
    display the current game phase somwehere
    display the id's of the players at all times.
- Add code smell analysis and tools like in sopra.
- Add more extensive tests.


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