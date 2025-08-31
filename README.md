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
    requirements:
    6.1 No player acts as a permanent server.
    6.2 Any player can host a lobby, but the game state is replicated.
    6.3 Game must continue if any peer (including host) disconnects.
    6.5 All messages must be authenticated and verifiable (Noise + signature).
    6.6 Consensus on game state must be maintained among peers.
7. message passing protocol (DONE? clients just broadcast messages.)
8. Peer Discovery
    8.1 Use libp2p to allow a player to advertise a lobby.
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
12. look at source code of dark forest zk game, get inspired by their zk-cuircits implementation and their zk debug console.
13. Add more extensive tests.


### Follow-up Thesis (outline only)

1. Literature study: Mental Poker & ZK-shuffle ([Wikipedia][5], [GitHub Docs][6])
2. Commitment & reveal channels ([doc.rust-lang.org][7])
3. Efficient N-party verifiable shuffling protocol
4. Circuit design (e.g. Halo 2 / Risc 0) and proof system benchmark ([MetaMask][8])
5. Punishment layer & escrow logic
6. Web3 wallet bridge (Metamask + ethers-rs) ([Medium][9])

+Bachelors thesis: Scope: Improved Efficient ZK Poker protocol + implementation
+1. Improved secure shuffling protocol.
+2. publishing of commitments to game participants.
+3. Verificatoin of commitments/proofs by other game participants.
+4. Penalties to Players who do not follow the agreed upon protocol.
+5. Metamask crypto wallet connections?
+6. Improved shuffle protocol.
+7. Choice of ZK circuit development stack.
+8.  Implementation of ZK circuits.
+9.  Proof of correctness for the protocol.
+10. Deck Shuffling and Card Dealing (Fairness)
+    Implement verifiable shuffling (e.g., Mental Poker):
+    Each player encrypts and permutes the deck using their public key.
+    Cards are revealed through cooperative decryption.
+    Prevents cheating and allows verifiable fairness.
+    Final thesis polishing: references, proofreading, formatting
+    Create README, reproducibility guide, and demo materials

---

## TODO Testing Strategy

* **State-machine property tests** using `proptest`
* **Network simulation**: spawn 3–5 in-process peers, replay canonical logs
* Continuous fuzzing hook via `cargo-fuzz` (optional)

Guides: Rust testing patterns ([Medium][4]) & open-source commit guidelines ([DEV Community][10]).

---

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
=======
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

====
relevant webpages:
https://zcash.github.io/halo2/index.html
https://docs.zkproof.org/reference#latest-version
