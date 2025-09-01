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

| Layer            | Crate / Module           |                                                              Purpose |
|------------------|--------------------------|---------------------------------------------------------------------:|
| GUI              | `egui_frontend`          |                                                   Local input/output |
| Game-Core        | `poker_core::game_state` |                        Deterministic state machine, hash-chained log |
| Network          | `p2p-net`                |                        libp2p swarm, gossip, CRDT-style merge buffer |
| Crypto Utilities | `poker_core::crypto`     |                                        Keys, signatures, commitments |
| Tests / CI       | `integration-tests`      | `cargo test` for running the tests, `clippy` for formatting, Actions |                                               |

### Key components:
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
- **Bottom Layer (Network/Crypto)**: libp2p handles peer discovery and gossip. All messages are signed using keys from the crypto module.
A CRDT-style merge buffer resolves conflicts by sorting messages timestamp-wise or by hash order.  
Flow: User action → Signed message → Broadcast → Receive & Validate → Append to Log → Step State → Update GUI.

### Architecture Diagram / Outline
The architecture can be visualized as a stack of interdependent crates, with the following flow:  
User action → Signed message → Broadcast → Receive & Validate → Append to Log → Step State → Update GUI.

![Architecture Diagram](https://raw.githubusercontent.com/random998/bachelors_project/refs/heads/main/docs/architecture3.svg)

### Implementation
#### Core Components
- **Game State Management (`game_state.rs`)**: Projection orchestrates updates. Example: `commit_step` applies transitions, computes hashes, and queues effects.
- **Messaging (`message.rs`)**: Signed messages ensure authenticity. Variants like StartGameNotify coordinate game start.
- **Player Management (`players_state.rs`)**: PlayerStateObjects handles turns with stable indices.
- **Network (p2p-net)**: service for the core game to send messages to other peers/players.
- **GUI Integration (`gui/src/game_view.rs`)**: Renders snapshots from Projection::snapshot().

#### poker_core crate
The `poker_core` crate serves as the foundational backbone of the P2P poker prototype, encapsulating
the deterministic state machine, networking primitives, cryptographic utilities, and poker-specific logic.
It is structured as a modular library crate with a `lib.rs` entry point that re-exports key modules for use in other crates
(e.g., `gui` for frontend integration or `integration-tests` for verification). The crate emphasizes separation of concerns:
Pure, immutable state transitions in `game_state.rs` ensure consensus between peers, while side-effectful components
(e.g. networking in `net/`) handle I/O (interfacing with peers and interfacing with the frontend).

##### Overall Structure and Flow:
- **Files/Directories**: Key files handle state (e.g. `game_state.rs`, `players_state.rs`), messages (`message.rs`),
crypto primitives (`crypto.rs`), poker rules (`poker.rs`), and utilities (`connection_stats.rs`, `timer.rs`).
 
- **Data Flow**: User Inputs or network messages -> Signed and validated (`crypto.rs` + `message.rs`) -> Appended to hash-chain log (`protocol/msg.rs` + `game_state.rs`)
-> State stepped forward deterministically (`protocol/state.rs`) -> Effects broadcast via network (`net/` traits and `runtime_bridge.rs`).
This lock-step flow ensures peers replicate the same state, but bugs arise from message reordering (e.g. in `handle_network_msg`).
 
**Interactions:**
- with `gui` crate: Provides `snapshot()` for rendering: receives UI events like bets.
- with `p2p-net` crate: Implements `P2pTransport` traits for libp2p integration.
- with tests: Modular pure functions (e.g. `step` in `ContractState` allow unit testing without network mocks).

Challenges in Code:
Flow is event-driven (e.g., tick() for pending effects), which works locally but fails in distributed simulations due to synchronization problems between the peers.

**Crate level Diagram (Data Flow)**:
```ASCII
+-------------+     +-----------------+     +-------------------+
| UI Events   | --> | SignedMessage   | --> | Network Broadcast |
| (from gui)  |     | (crypto.rs +    |     | (net/runtime_     |
+-------------+     |  message.rs)    |     |  bridge.rs)       |
                    +-----------------+     +-------------------+
                            |                        ^
                            v                        |
                    +-----------------+              |
                    | Log Append &    | <----------- +
                    | State Step      |
                    | (game_state.rs +
                    |  protocol/state.rs)
                    +-----------------+
                            |
                            v
                    +-----------------+
                    | GUI Snapshot    |
                    | (game_state.rs) |
                    +-----------------+

```
Below are overviews of major files/modules, grouped logically.

#### `lib.rs`
**Purpose:** Crate entry point; re-exports modules for external use; Exposes APIs for the entire application, no internal flow.

#### `game_state.rs`
- **Purpose:** Manages the replicated state machine via `Projection` (mutable view of the state with local data) and `ContractState` (pure, immutable core state).
Handles phases like dealing/betting, log appends, and GUI snapshots.

- **Structure/Flow:** `Projection` struct holds state (e.g. `hash_chain`, `contract`)
and methods like `commit_step` (appends logs, steps state)
and `handle_network_msg` (processes messages, e.g. sync or transitions).
Flow: Receive msg -> Validate/commit -> Update state -> Queue effects for broadcast.

- **Key code snippet** (`commit_step` function, Showing Log Append and State Step):
```rust
fn commit_step(&mut self, payload: &Transition) -> anyhow::Result<()> {
    let StepResult { next, effects } = contract::step(&self.contract, payload);
    let next_hash = contract::hash_state(&next);
    let entry = LogEntry::with_key(self.hash_head.clone(), payload.clone(), next_hash.clone(), self.peer_id());
    let signed = SignedMessage::new(&self.key_pair, NetworkMessage::ProtocolEntry(entry.clone()));
    self.send(signed)?;
    self.contract = next;
    self.hash_head = next_hash;
    self.hash_chain.push(entry);
    self.pending_effects.extend(effects.into_iter().filter_map(|e| match e { Effect::Send(m) => Some(m) }));
    Ok(())
}
```
- **Interactions**: Calls `protocol::step` for pure transitions; sends via `net`; provides `snapshot()` to GUI.
Bugs here: Hash mismatches from unsynced effects between the different instances of the peers.

#### `players_state.rs`
- **Purpose**: Manages player records (e.g. chips, actions) in a vec-based wrapper (`PlayerStateObjects`) for turn logic and consensus.
- **Structure/Flow**: Struct with methods like `activate_next_player` (cycles active index which indicates who's players turn it is) and `place_bet` (updates chips/action).
Flow: state step calls these to enforce rules (e.g. advance turns after bets).
- **Key snippet** (activate_next_player) method:
```rust
pub fn activate_next_player(&mut self) {
    if self.count_active() > 0 && self.active_player().is_some() {
        let active_player = self.active_idx.take().unwrap();
        let iter = self.players.iter().enumerate().cycle().skip(active_player + 1).take(self.players.len() - 1);
        for (pos, p) in iter {
            if p.is_active && p.chips > Chips::ZERO {
                self.active_idx = Some(pos);
                break;
            }
        }
    }
}
``` 
- **Interactions**: Used by `game_state.rs` for game phase updates (betting phase, reveal phase, game start phase etc....)

#### `message.rs` 
- **Purpose**: Defines network/UI messages (e.g. `SignedMessage`, `NetworkMessage` variants like `ProtocolEntry`) and player actions.
- **Structure/Flow**: Enums for messages/actions; `SignedMessage` struct handles signing/verification of mesages.
Flow: UI events -> Messages -> Broadcast; received messages trigger state steps.
- **Key snippet** (SignedMessage Struct): 
```rust
#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct SignedMessage {
    payload: Arc<Payload>,
    sig: Signature,
}
impl SignedMessage {
    pub fn new(key_pair: &KeyPair, msg: NetworkMessage) -> Self {
        let sig = key_pair.secret().sign(&msg);
        Self { sig, payload: Arc::new(Payload { msg, public_key: key_pair.public() }) }
    }
}
```
- **Interactions**: Used in `crypto.rs` for signing; parsed in `game_state.rs` handlers.

#### `crypto.rs`
- **Purpose**: Provides cryptographic primitives (e.g., Ed25519 keys, signatures) for message authenticity.
- **Structure/Flow:** Structs like KeyPair, PeerId; methods for signing/verifying. Flow: All messages signed before send; verified on receive.
- **Key Snippet** (KeyPair Generation):
```rust
pub struct Deck(Vec<Card>);
impl Deck {
    pub fn shuffled(rng: &mut impl Rng) -> Self {
        let mut cards = (0..52).map(Card::from).collect::<Vec<_>>();
        cards.shuffle(rng);
        Deck(cards)
    }
    pub fn deal(&mut self) -> Card {
        self.0.pop().unwrap()
    }
}
```
- **Interactions:** Called deterministically in game_state.rs for fair play.

#### `protocol/Subdirectory`
- **Purpose:** Defines consensus protocol (e.g., state transitions, log entries).
- **Files:** msg.rs (transitions like Transition::Bet), state.rs (ContractState, step function).
- **Structure/Flow:** ContractState holds log/phase; step applies payloads, returns next state/effects. Flow: Core of replication—messages trigger steps.
- **Key Snippet** (From state.rs - step Function):
```rust
#[must_use]
pub fn step(prev: &ContractState, msg: &Transition,) -> StepResult {
    let mut st = prev.clone();
    let mut out = Vec::new();

    match msg {
        Transition::StartGameBatch(batch,) => {
            // Verify: Complete, sorted, valid
            let expected_senders: Vec<PeerId,> =
                st.players.keys().copied().collect();

            let mut batch_senders_sorted: Vec<PeerId,> = batch
                .iter()
                .map(super::super::message::SignedMessage::sender,)
                .collect();
            batch_senders_sorted.sort_by_key(std::string::ToString::to_string,);

            let mut expected_senders_sorted: Vec<PeerId,> =
                expected_senders.clone();
            expected_senders_sorted
                .sort_by_key(std::string::ToString::to_string,);

            if batch_senders_sorted != expected_senders_sorted
                || batch_senders_sorted.len() != expected_senders.len()
            {
                info!(
                    "invalid startGameBatch message, rejecting:\n\
                    batch_senders_len: {batch_senders_sorted:#?},\n\
                    expected_senders_len: {expected_senders_sorted:#?}"
                );
                return StepResult {
                    next:    prev.clone(),
                    effects: vec![],
                };
            }

            // Verify each signature and fields match
            for sm in batch {
/*                if !sm.verify() {
                    return StepResult {
                        next:    prev.clone(),
                        effects: vec![],
                    };
                }
*/                // Apply: Set flags
                if let Some(p,) = st.players.get_mut(&sm.sender(),) {
                    p.has_sent_start_game_notification = true;
                }
            }
            
            // All good: Advance phase
            if st
                .players
                .values()
                .all(|p| p.has_sent_start_game_notification,)
            {
                st.phase = HandPhase::StartingHand;
            }
            // add startGameBatch message to effects, since we want to send it
            // to our peers.else {
            let eff = Effect::Send(msg.clone(),);
            out.push(eff,);
        },
        Transition::JoinTableReq {
            player_id,
            table: _table,
            chips,
            nickname,
        } => {
            st.players.insert(
                *player_id,
                PlayerPrivate::new(*player_id, nickname.clone(), *chips,),
            );

            if st.players.len() >= st.num_seats {
                st.phase = HandPhase::StartingGame;
            }
        },
        Transition::DealCardsBatch(batch,) => {
            // Verify: Complete, sorted, valid for active players
            let expected_receivers: Vec<PeerId,> = st
                .players
                .values()
                .filter(|p| p.is_active && p.chips > Chips::ZERO,)
                .map(|p| p.peer_id,)
                .collect();
            let mut batch_receivers_sorted: Vec<PeerId,> =
                batch.iter().map(|dc| dc.player_id,).collect();
            batch_receivers_sorted
                .sort_by_key(std::string::ToString::to_string,);
            let mut expected_receivers_sorted = expected_receivers.clone();
            expected_receivers_sorted
                .sort_by_key(std::string::ToString::to_string,);
            if batch_receivers_sorted != expected_receivers_sorted
                || batch.len() != expected_receivers.len()
            {
                info!("Invalid DealCardsBatch; rejecting");
                return StepResult {
                    next:    prev.clone(),
                    effects: vec![],
                };
            }
            // Apply: Update hole_cards for each
            for dc in batch {
                if let Some(player,) = st.players.get_mut(&dc.player_id,) {
                    player.hole_cards = Cards(dc.card1, dc.card2,);
                }
            }
            // Add to effects if needed (e.g., broadcast)
            let eff = Effect::Send(msg.clone(),);
            out.push(eff,);
        },
        Transition::ActionRequest { .. } => {
            todo!()
        },
        Transition::Ping => {
            todo!()
        },
        _ => {
            todo!()
        },
    }
    StepResult {
        next:    st,
        effects: out,
    }
}

```
- **Interactions:** Central to game_state.rs; ensures hash-chained consensus.
- Other Utility Files
`connection_stats.rs`: Tracks network metrics (e.g., RTT); used in Projection for debugging.
`timers.rs`: Manages timeouts (e.g., action delays); integrates with tick() in `game_state.rs`.
`zk.rs`: Stub for ZK proofs (e.g., shuffle verification); minimal now, for thesis extension.
`net/ Subdirectory`: Traits (P2pTransport) and runtime bridge for libp2p; flow: Abstracts sending/receiving for `game_state.rs`.

**Summary of Crate Strengths/Issues:**
Modularity enables easy swaps (e.g., add Raft), but event-driven flow (e.g., tick()) causes sync bugs in tests.

#### poker_eval crate
Assuming a separate `poker_eval` crate (or integrated module), it handles poker-specific logic such as hand ranking and evaluation.
Using libraries like `poker` or custom implementations, it provides functions like `evaluate_hand(cards: &[Card]) -> HandRank`.
This ensures deterministic outcomes across peers, with tests verifying standard poker rules (e.g., royal flush beats straight).

#### gui crate
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
- Failing tests involve state divergence: .Hash mismatches in distributed mode due to out-of-order messages, Sync failures under simulated delays, Attempted fixes: Timeouts/retries in gossip; logging replays.\
- Many functions / scenarios still remain untested. Maybe it would be handy to introduce a notion of `test coverage` but I do not know anything in regard to that... \
I am not satisfied with the current state of the test structure (there is neither a plan on how to test / what should be tested, nor do I know how tests
should be structured in general).
- I am missing a structured approach for creating a *working* fault-tolerant distributed state machine, don't know where existing solutions can be found and how they could be used/integrated.

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
How do I approach implementing a byzantinte fault-tolerant state machine?

### Organization
I had many points along the way where I got lost and just coded along without a
real plan. This was very time-consuming. I think I have invested well over 180
hours into the bachelors project already.
 

### Conclusion and Next Steps
The prototype demonstrates viable P2P poker basics but requires bug fixes for reliability.  
I propose a meeting to discuss the above questions, refine scope.

## References
[1] Shamir, Rivest, Adleman. "Mental Poker" (1979).  
[2] Shapiro et al. "Conflict-Free Replicated Data Types" (2011).  
[3] Ongaro, Ousterhout. "In Search of an Understandable Consensus Algorithm (Raft)" (2014).  
[4] libp2p Documentation.  
[5] Stateright GitHub Repository.  
[6] Halo2: Zcash Implementation.  
[7] Yin et al. "HotStuff: BFT Consensus with Linearity and Responsiveness" (2019).
