## Scope

This repository hosts the **prototype implementation** of a **deterministic, peer-to-peer poker engine written in Rust**.
The prototype focuses on:

* lock-step hash-chain replication for game-state consensus
* a minimal GUI (egui) front-end
* network transport over libp2p

**Zero-knowledge (ZK) shuffling and proof verification are *out-of-scope* for this project**; they will be designed and integrated during the follow-up Bachelor thesis.

---

## Architecture (Prototype)

| Layer                | Crate / Module                                       | Purpose                                                        |
| -------------------- | ---------------------------------------------------- | -------------------------------------------------------------- |
| **GUI**              | `egui_frontend`                                      | Local input/output                                             |
| **Game-Core**        | `poker_core::game_state`                             | Deterministic state machine, hash-chained log                  |
| **Network**          | `poker_core::net`                                    | libp2p swarm, gossip, CRDT-style merge buffer ([hackmd.io][2]) |
| **Crypto Utilities** | `poker_core::crypto`                                 | Keys, signatures, commitments                                  |
| **Tests / CI**       | `cargo test`, `clippy`, GitHub Actions ([GitHub][3]) |                                                                |
=======

1. p2p game state synchronization (blockchain?):
requirements:\
1.1 No player acts as a permanent server.
1.2 Any player can host a lobby, but the game state is replicated.
1.3 Game must continue if any peer (including host) disconnects.
1.4 All messages must be authenticated and verifiable (Noise + signature).
1.5 Consensus on game state must be maintained among peers.

2. message passing protocol (DONE? clients just broadcast messages.)
3. Decentralized Game State
    3.1 Replicate the full game state (table, pot, players, deck hash, etc.) across all peers.
    3.2 Each peer runs a local state machine.
    3.3 Use signed, timestamped messages and broadcast every state transition.
4. Consensus Protocol?
    4.1 Use a deterministic protocol to decide turn order and resolve actions:
    E.g., fixed round-robin for turns.
    4.2 Each action includes a signed ActionMessage.
    4.3 Optionally use a simple majority or BFT protocol for validating transitions.
    4.4 Implement a rollback/timeout system for stalled or invalid actions.

5. look at C open source web poker implementation and compare with my own! Incorporate parts of the code if feasible.
6. look at source code of dark forest zk game, get inspired by their zk-cuircits implementation and their zk debug console.
7. Add more extensive tests.


### Follow-up Thesis (outline only)

1. Literature study: Mental Poker & ZK-shuffle.
2. Commitment & reveal channels
3. Efficient N-party verifiable shuffling protocol
4. Proof of correctness for the protocol.
5. Literature study: different types of zk circuits & proof systems.
6. Circuit design (e.g. Halo 2 / Risc 0) and proof system benchmark.
7. Implementation of ZK circuits.
8. Punishment layer & escrow logic
9. Verificatoin of commitments/proofs by other game participants.
10. Penalties to Players who do not follow the agreed upon protocol.
11. Final thesis polishing: references, proofreading, formatting

---

## Testing Strategy
TODO:
think about what kind of testing strategy should be deployed to test the system.
Also the tests are structured very badly at the moment. There is need for proper organization.

* **State-machine tests**: maybe use the stateright formal checker?
* **Network simulation**: spawn 3–5 in-process peers, replay canonical logs
* **unit tests**: thorough testing of individual crates
## Documentation

* `docs/report.md` – living project report (export to PDF on tag)
* Design notes deriving from the academic references on Mental Poker ([ResearchGate][1]) and CRDT-based game engines ([hackmd.io][2])

---

[1]: https://www.researchgate.net/publication/221354695_A_Zero-Knowledge_Poker_Protocol_That_Achieves_Confidentiality_of_the_Players%27_Strategy_or_How_to_Achieve_an_Electronic_Poker_Face?utm_source=chatgpt.com "(PDF) A Zero-Knowledge Poker Protocol That Achieves ..."
[2]: https://hackmd.io/%40nmohnblatt/SJKJfVqzq?utm_source=chatgpt.com "a zero knowledge library for Mental Poker (and all card games)"
[3]: https://github.com/rust-lang/rust-clippy?utm_source=chatgpt.com "rust-lang/rust-clippy: A bunch of lints to catch common ... - GitHub"
[4]: https://medium.com/coinmonks/commit-reveal-scheme-in-solidity-c06eba4091bb?utm_source=chatgpt.com "Commit-Reveal scheme in Solidity. What is it? | by Srinivas Joshi"
[5]: https://en.wikipedia.org/wiki/Mental_poker?utm_source=chatgpt.com "Mental poker - Wikipedia"
[6]: https://docs.github.com/en/actions/how-tos/use-cases-and-examples/building-and-testing/building-and-testing-rust?utm_source=chatgpt.com "Building and testing Rust - GitHub Docs"
[7]: https://doc.rust-lang.org/book/ch11-03-test-organization.html?utm_source=chatgpt.com "Test Organization - The Rust Programming Language"
[8]: https://metamask.io/news/polysnap-invoking-polywrap-wasm-wrappers-on-the-fly?utm_source=chatgpt.com "Polysnap: Invoking Polywrap Wasm Wrappers on the fly - MetaMask"
[9]: https://medium.com/%40kaishinaw/connect-metamask-with-ethers-js-fc9c7163fd4d?utm_source=chatgpt.com "Connect Metamask with Ethers.js - Medium"

====
relevant webpages:\
https://zcash.github.io/halo2/index.html \
https://docs.zkproof.org/reference#latest-version \
