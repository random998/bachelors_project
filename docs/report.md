# Prototype Implementation of a Deterministic Peer-to-Peer Poker Engine in Rust Using Lock-Step Hash-Chain Replication}

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
#### Poker mechanics and distibuted systems
Poker involves deterministic rules (e.g., hand evaluation using crates like {poker_eval}). For P2P, consensus is key: lock-step replication ensures peers execute identical inputs in order, verified via hash-chains (inspired by CRDTs [2] and Raft-like models [3]).
Related work:

Mental Poker [1]: Theoretical foundations for fair card dealing without revelation.
libp2p [4]: Used for swarm-based P2P networking.
Stateright [5]: Considered for model-checking the state machine (not yet integrated).

### Technologies
Rust: Chosen for safety and performance in concurrent systems.
libp2p: Handles peer discovery, gossip, and message propagation.
egui: Simple GUI for user input/output.
blake3/ahash: For hashing state and logs.

Literature on ZK (deferred): SNARKs (e.g., Halo2 [6]) for verifiable shuffles, and byzantine fault tolerance (BFT) in protocols like HotStuff [7].

### Design and Architecture
The system follows a layered design (as per README):

Layer,Crate/Module,Purpose
GUI,gui,egui frontend for input/output.
Game-Core,core::{game_state}, Deterministic state machine with hash-chained log.
Network,core::net / p2p-net,libp2p swarm for gossip and CRDT-style merge buffer.
Crypto,core::crypto,"Keys, signatures, commitments (ZK stubs)."

Key components:
- Projection: A mutable view of the canonical ContractState, enriched with local data (e.g., RNG, connection stats). Handles updates, message processing, and GUI snapshots.
- ContractState: Pure, immutable state machine for consensus. Uses step function to apply transitions and compute effects.
- Hash-Chain Replication: Each LogEntry (with {prev_hash}, payload, {next_hash}) ensures ordered, verifiable execution. Messages like ProtocolEntry append to the chain.
- Networking: libp2p swarm manages connections; gossip propagates signed messages. Sync mechanisms (e.g., SyncReq/SyncResp) bootstrap new peers.

### Key Design Decisions

Lock-Step Replication: Peers broadcast inputs (e.g., bets), append to log, and verify hashes. Simpler than full Raft for prototype but assumes semi-synchronous networks.
Deterministic RNG: Seeded StdRng for reproducible dealing when local peer acts as dealer.
Phases: HandPhase enum (e.g., StartingGame, PreflopBetting) drives transitions.
Error Handling: Custom errors (e.g., TableJoinError) for user-facing issues.

Potential Issues: The architecture assumes honest peers and no byzantine faults, which ZK would address. Current merge buffer may not handle all reordering cases, leading to bugs.

### Architecture Diagram / Outline
TODO



### Implementation
#### Core components
- Game State Management ({game_state.rs}): Projection orchestrates updates. Example: {commit_step} applies transitions, computes hashes, and queues effects.
- Messaging (message.rs): Signed messages ensure authenticity. Variants like StartGameNotify coordinate game start.
- Player Management ({players_state.rs}): PlayerStateObjects handles turns with stable indices.
- GUI integration: ({gui/src/game_view.rs}): Renders snapshots from Projection::snapshot().

#### core crate
TODO

#### eval crate
TODO

#### gui crate
TODO

#### integration tests crate
TODO

### Progress Highlights
Local gameplay: Dealing, betting, showdown work deterministically.
P2P Sync: New peers request and replay chains from seed peers.
Batch Processing: E.g., StartGameBatch sorts notifications for consensus.

### Evaluation and Testing
### Current Testing Approach
- unit tests: basic for card evaluation (eval crate).
- Integration: Manual simulation with 3-in-process peers replaying logs.

### Issues and Bugs
Failing tests involve state divergence:

Hash mismatches in distributed mode due to out-of-order messages.
Sync failures under simulated delays.
Attempted fixes: Timeouts/retries in gossip; logging replays.
    
### Structure Suggestions/Questions:
Unit: Test pure functions (e.g., step in ContractState).
Integration: Use libp2p's in-memory transport for multi-peer sims.
Property: Assert invariants like "hashes match after replay" via proptest.

### Open Questions:
Is the architecture correct?
It works locally but may need Raft-like leader election for robustness.

\section{Challenges and Open Questions}
\subsection{Scope of Bachelor's Project}

What is the required scope? With the current prototype (functional local/P2P basics, excluding ZK), is this sufficient for completion, or must distributed bugs be fully resolved?

### Bug Resolution

I lack strategies for debugging distributed state divergence. Suggestions tried: Detailed logging, chain replays. How to systematically solve these (e.g., using Stateright for model checking)?

### Testing Structure
How to organize tests? E.g., separate crates for unit/integration? Best practices for testing P2P systems in Rust?

### Architecture Validation
Is the lock-step hash-chain approach sound? Potential flaws: Assumes synchrony; vulnerable to partitions without BFT.

### Finalizing in a Subsequent Thesis
Can this prototype be extended in a master's thesis? Ideas:

Integrate ZK circuits (e.g., Halo2 for verifiable shuffles).
Formal specification of shuffling algorithm.
Collusion prevention (e.g., via commitments).
Consensus analysis/BFT.
How to handle adverse player behaviour?
Player audits of hash logs for ZK correctness.

### Alignment with Supervisor's Interests and Personal Goals
Your expertise is in theoretical cryptography (e.g., proofs for mental card games).
I aim to focus on software engineering/architecture, with literature analysis on ZK-SNARKs, consensus, and blockchains.
I'd like to include a formal specification of the distributed state machine/ZK primitives/cheating prevention, but prioritize SE skills (e.g., refactoring, testing, scalability).
How can we balance this?

### Conclusion and Next Steps
The prototype demonstrates viable P2P poker basics but requires bug fixes for reliability.
I propose a meeting to discuss the above questions, refine scope, and plan thesis extensions.
