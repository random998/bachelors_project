# Prototype Implementation of a Deterministic Peer-to-Peer Poker Engine in Rust Using Lock-Step Hash-Chain Replication

## Abstract
This report summarizes the progress on my bachelor's project, which involves developing a prototype for a deterministic peer-to-peer (P2P) poker application in Rust. The prototype implements lock-step hash-chain replication for game-state consensus, a minimal egui-based GUI, and libp2p for network transport. Zero-knowledge (ZK) shuffling and proof verification are deferred to a potential follow-up thesis. Key achievements include a functional local gameplay loop and basic P2P synchronization for 3 peers. However, challenges remain in debugging distributed state machine bugs, structuring tests, and validating the architecture. This report outlines the current implementation, encountered issues, and open questions regarding scope, bug resolution, testing, architecture, and future extensions. It serves as a basis for discussion on project completion and alignment with theoretical cryptography interests.

## Introduction
### Problem Statement
Traditional online poker relies on centralized servers, introducing trust issues, single points of failure, and potential for cheating. This project explores a P2P alternative where peers collaboratively maintain a deterministic game state using hash-chain replication, ensuring consensus without a central authority. The prototype focuses on core mechanics like dealing, betting, and hand evaluation, with ZK elements (e.g., fair shuffling) planned for later.

Inspired by Mental Poker protocols [1], the system aims for fairness and resilience in a decentralized environment. The current scope excludes ZK proofs, focusing instead on the replicated state machine and P2P networking.

### Objectives
- Implement a deterministic poker state machine with lock-step replication.
- Integrate libp2p for P2P communication and egui for a minimal GUI.
- Achieve basic gameplay for 3-5 peers in simulated networks.
- Identify and document challenges for a potential thesis extension.

### Current Status
The prototype runs locally and supports basic P2P interactions. However, distributed tests fail due to state divergence under network delays. This report details the architecture, implementation highlights, and unresolved issues.

### Background and Related Work
#### Poker Mechanics and Distributed Systems
Poker involves deterministic rules (e.g., hand evaluation using crates like `poker_eval`). For P2P, consensus is key: lock-step replication ensures peers execute identical inputs in order, verified via hash-chains (inspired by CRDTs [2] and Raft-like models [3]).

Related work:
- Mental Poker [1]: Theoretical foundations for fair card dealing without revelation.
- libp2p [4]: Used for swarm-based P2P networking.
- Stateright [5]: Considered for model-checking the state machine (not yet integrated).

### Technologies
Rust: Chosen for safety and performance in concurrent systems.
libp2p: Handles peer discovery, gossip, and message propagation.\
egui: Simple GUI for user input/output.\
blake3/ahash: For hashing state and logs.\

### Design and Architecture
The system follows a layered design:

| Layer            | Crate / Module                                 |                                       Purpose |
|------------------|------------------------------------------------|----------------------------------------------:|
| GUI              | `egui_frontend`                                |                            Local input/output |
| Game-Core        | `poker_core::game_state`                       | Deterministic state machine, hash-chained log |
| Network          | `p2p-net`                                      | libp2p swarm, gossip, CRDT-style merge buffer |
| Crypto Utilities | `poker_core::crypto`                           |                 Keys, signatures, commitments |
| Tests / CI       | `cargo test` for running the tests, `clippy` for formatting, Actions |                                               |

Key components:
- **Projection**: A mutable view of the canonical ContractState, enriched with local data (e.g., RNG, connection stats). Handles updates, message processing, and GUI snapshots.
- **ContractState**: Pure, immutable state machine for consensus. Uses step function to apply transitions and compute effects.
- **Hash-Chain Replication**: Each LogEntry (with `prev_hash`, payload, `next_hash`) ensures ordered, verifiable execution. Messages like ProtocolEntry append to the chain.
- **Networking**: libp2p swarm manages connections; gossip propagates signed messages. Sync mechanisms (e.g., SyncReq/SyncResp) bootstrap new peers.

### Key Design Decisions
Lock-Step Replication: Peers broadcast inputs (e.g., bets), append to log, and verify hashes. Simpler than full Raft for prototype but leads to errors/bugs.  
Phases: HandPhase enum (e.g., StartingGame, PreflopBetting) drives transitions.  
Error Handling: Custom errors (e.g., TableJoinError) for user-facing issues.  
Potential Issues: The architecture assumes honest peers and no byzantine faults, which ZK would address. Current merge buffer may not handle all reordering cases, leading to bugs.

### Architecture Diagram / Outline
The architecture can be visualized as a stack:  
- **Top Layer (GUI)**: egui renders game views based on snapshots from the Projection. User inputs (e.g., bet actions) are translated into signed messages and broadcast via the network layer.  
- **Middle Layer (Game-Core)**: The ContractState maintains an immutable hash-chained log. Incoming messages are validated, appended to the log, and used to step the state machine forward. Effects (e.g., update pot size) are computed deterministically.  
- **Bottom Layer (Network/Crypto)**: libp2p handles peer discovery and gossip. All messages are signed using keys from the crypto module. A CRDT-style merge buffer resolves conflicts by sorting messages timestamp-wise or by hash order.  
Flow: User action → Signed message → Broadcast → Receive & Validate → Append to Log → Step State → Update GUI.

### Architecture Diagram / Outline
The architecture can be visualized as a stack of interdependent crates, with the following flow:  
User action → Signed message → Broadcast → Receive & Validate → Append to Log → Step State → Update GUI.

[Architecture Diagram](architecture.svg)

### Implementation
#### Core Components
- **Game State Management (`game_state.rs`)**: Projection orchestrates updates. Example: `commit_step` applies transitions, computes hashes, and queues effects.
- **Messaging (`message.rs`)**: Signed messages ensure authenticity. Variants like StartGameNotify coordinate game start.
- **Player Management (`players_state.rs`)**: PlayerStateObjects handles turns with stable indices.
- **GUI Integration (`gui/src/game_view.rs`)**: Renders snapshots from Projection::snapshot().

#### Core Crate
The `poker_core` crate serves as the backbone, containing modules for game_state, net, and crypto. It defines the deterministic state machine in `game_state.rs`, where the ContractState struct holds the hash-chained log and applies transitions via a `step` method.\
Network integration in `net.rs` uses libp2p's Swarm to manage behaviors like gossipsub for message propagation.\
Crypto utilities in `crypto.rs` provide Ed25519 signatures and basic commitments (with ZK stubs for future expansion).\
The crate is designed for modularity, allowing easy testing of pure functions like state transitions.

#### Eval Crate
Assuming a separate `poker_eval` crate (or integrated module), it handles poker-specific logic such as hand ranking and evaluation. Using libraries like `poker` or custom implementations, it provides functions like `evaluate_hand(cards: &[Card]) -> HandRank`. This ensures deterministic outcomes across peers, with tests verifying standard poker rules (e.g., royal flush beats straight).

#### GUI Crate
The `egui_frontend` crate implements a minimal GUI using egui.\
In `main.rs`, it sets up an egui context and renders components like player hands, pot display, and action buttons (e.g., fold, call, raise).\
It interfaces with the core crate by polling the Projection for snapshots and sending user actions as messages. The GUI is lightweight, focusing on functionality over aesthetics, with basic event handling for real-time updates.

#### Integration Tests Crate
A dedicated `tests` directory or crate contains integration tests.\
Using Rust's built-in testing framework, it spawns multiple in-process peers via libp2p's memory transport.\
Scenarios simulate gameplay: e.g., 3 peers join, deal cards, place bets, and verify final state hashes match.\
Current tests cover local loops but highlight failures in distributed setups.\

### Progress Highlights
Local gameplay: Dealing, betting, showdown work deterministically.\
P2P Sync: New peers request and replay chains from seed peers.\

Current message reordering is `solved` (not really) via Batch Processing: E.g., StartGameBatch sorts notifications for consensus.
I spent a lot of time trying to get the distributed system to work, I
acknowledge that I am not able to get it to work and need to look for existing
solutions/frameworks/libraries to integrate into the application.

### Evaluation and Testing

### Current Testing Approach
- **Unit Tests**: Basic for card evaluation (eval crate).
- **Integration**: Manual simulation with 3-in-process peers replaying logs.


### Issues and Bugs
Many functions / scenarios still remain untested. Maybe it would be handy to
introduce a notion of `test coverage` but I do not know anything in regards to
that...
Failing tests involve state divergence: .Hash mismatches in distributed mode due
to out-of-order messages, Sync failures under simulated delays, Attempted fixes: Timeouts/retries in gossip; logging replays.\
I am not satisfied with the current state of the test structure (there is
neither a plan on how to test / what should be tested, nor do I know how tests
should be structured in general).

### Structure Suggestions/Questions
Unit: Test pure functions (e.g., step in ContractState).  
Integration: Use libp2p's in-memory transport for multi-peer sims.  
Property: Assert invariants like "hashes match after replay" via proptest.


### Open Questions
#### Scope of Bachelor's Project
What is the required scope? With the current prototype (functional local/P2P basics, excluding ZK), is this sufficient for completion, or must distributed bugs be fully resolved?

#### Bug Resolution
I lack strategies for debugging distributed state divergence.
Suggestions tried: Detailed logging, chain replays. How to systematically solve these (e.g., using Stateright for model checking)?

#### Testing Structure
How to organize tests? E.g., separate crates for unit/integration? Best practices for testing P2P systems in Rust?

#### Architecture Validation
Is the lock-step hash-chain approach sound? Potential flaws: Assumes synchrony; vulnerable to partitions without BFT.

### Organization
I had many points along the way where I got lost and just coded along without a
real plan. This was very time consuming. There must be better approaches...
 
#### Finalizing in a Subsequent Thesis
Can this prototype be extended in a master's thesis? Ideas:  
Integrate ZK circuits (e.g., Halo2 for verifiable shuffles).  
Formal specification of shuffling algorithm.  
Collusion prevention (e.g., via commitments).  
Consensus analysis/BFT.  
How to handle adverse player behaviour?  
Player audits of hash logs for ZK correctness.

#### Alignment with Supervisor's Interests and Personal Goals
Your expertise is in theoretical cryptography (e.g., proofs for mental card games).  
I aim to focus on software engineering/architecture, with literature analysis on ZK-SNARKs, consensus, and blockchains.  
I'd like to include a formal specification of the distributed state machine/ZK primitives/cheating prevention, but prioritize SE skills (e.g., refactoring, testing, scalability).  
How can we balance this?

### Conclusion and Next Steps
The prototype demonstrates viable P2P poker basics but requires bug fixes for reliability.  
I propose a meeting to discuss the above questions, refine scope, and plan thesis extensions.

## References
[1] Shamir, Rivest, Adleman. "Mental Poker" (1979).  
[2] Shapiro et al. "Conflict-Free Replicated Data Types" (2011).  
[3] Ongaro, Ousterhout. "In Search of an Understandable Consensus Algorithm (Raft)" (2014).  
[4] libp2p Documentation.  
[5] Stateright GitHub Repository.  
[6] Halo2: Zcash Implementation.  
[7] Yin et al. "HotStuff: BFT Consensus with Linearity and Responsiveness" (2019).
