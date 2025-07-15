## Scope

This repository hosts the **prototype implementation** of a **deterministic, peer-to-peer poker engine written in Rust**.
The prototype focuses on:

* lock-step hash-chain replication for game-state consensus ([ResearchGate][1])
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

---

## Quick Start

```bash
# 1. Install Rust tool-chain
rustup toolchain install stable

# 2. Clone & build
git clone https://github.com/your-handle/zk-poker.git
cd zk-poker
cargo run --example two_peers
```

To run the headless integration-tests:

```bash
cargo test --workspace      # unit & property tests
cargo clippy -- -D warnings  # lint gate in CI
```

See the test-writing guide ([Medium][4]).

---

## Roadmap & Milestones

### Prototype (Bachelor project)

| Milestone                                 | Target date | Done when…                                          |
| ----------------------------------------- | ----------- | --------------------------------------------------- |
| **M1: UI bug-fix pass**                   | 17.07.2025  | All UI tasks in `issues/ux` closed                  |
| **M2: Deterministic state-machine tests** | 18.07.2025  | Coverage ≥ 80 % on `game_state.rs`; fuzz pass green |
| **M3: Contract hash-chain review**        | 19.07.2025  | Formal walk-through with advisor                    |
| **M4: Project report draft**              | 20.07.2025  | 8–10 pp PDF pushed to `docs/`                       |
| **M5: Submission & tag `v0.1.0`**         | 20.07.2025  | Repo archived & email to exam office                |

### Follow-up Thesis (outline only)

1. Literature study: Mental Poker & ZK-shuffle ([Wikipedia][5], [GitHub Docs][6])
2. Commitment & reveal channels ([doc.rust-lang.org][7])
3. Efficient N-party verifiable shuffling protocol
4. Circuit design (e.g. Halo 2 / Risc 0) and proof system benchmark ([MetaMask][8])
5. Punishment layer & escrow logic
6. Web3 wallet bridge (Metamask + ethers-rs) ([Medium][9])

---

## Testing Strategy

* **State-machine property tests** using `proptest`
* **Network simulation**: spawn 3–5 in-process peers, replay canonical logs
* Continuous fuzzing hook via `cargo-fuzz` (optional)

Guides: Rust testing patterns ([Medium][4]) & open-source commit guidelines ([DEV Community][10]).

---

## Documentation

* `docs/report.md` – living project report (export to PDF on tag)
* API docs with `cargo doc --open`
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
