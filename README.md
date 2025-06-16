Bachelors project Scope: Development of a p2p zero-knowledge poker game in Rust.

Roadmap:

1. web client:
    1.1 first starter screen application using egui.
    1.2 webclient structure / screen organization.
    1.3 window/log/console that logs every action that happens & every interaction & message with any other client.    

2. poker hand evaluator.
3. p2p lobby creation / message exchange / reconnects / disconnects.
4. account/ID management.
5. Betting logic.
6. p2p game state synchronization (blockchain?):
    requirements:
    6.1 No player acts as a permanent server.
    6.2 Any player can host a lobby, but the game state is replicated.
    6.3 Game must continue if any peer (including host) disconnects.
    6.5 All messages must be authenticated and verifiable (Noise + signature).
    6.6 Consensus on game state must be maintained among peers.
7. message passing protocol.
8. Lobby Advertisement (Peer Discovery)
    8.1 Use multicast, DNS-SD, libp2p, or DHT to allow a player to advertise a lobby.
    8.2 Peers can discover open games and connect via Noise-encrypted WebSockets.
    8.3 No peer should be a permanent coordinator.
9. Decentralized Game State
    9.1 Replicate the full game state (table, pot, players, deck hash, etc.) across all peers.
    9.2 Each peer runs a local state machine.
    9.3 Use signed, timestamped messages and broadcast every state transition.
10. Consensus on Game Progression
    10.1 Use a deterministic protocol to decide turn order and resolve actions:
    E.g., fixed round-robin for turns.
    10.2 Each action includes a signed ActionMessage.
    10.3 Optionally use a simple majority or BFT protocol for validating transitions.
    10.4 Implement a rollback/timeout system for stalled or invalid actions.
11. Resilience and Handoff
    In case a peer disconnects:
        Game state is preserved by the others.
        If a turn is missed, timeout and default action (fold/check).
    Each peer should buffer recent state transitions and have a retry/rejoin protocol.

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



WEEKLY PLAN:
Revised Weekly Plan
Weeks 1–2: Jun 16–29

Focus: GUI + Core Infrastructure

    egui startup screen + console logger
    Project structure based on freezeout
    Noise-encrypted WebSocket P2P setup
    Local log/debug window working

Weeks 3–4: Jun 30 – Jul 13

Focus: Poker Engine + P2P Core

    Poker hand evaluator (testing all hand types)
    P2P lobby creation and handshake
    Message authentication + account management

Thesis Begins (Jul 1):

    Read Mental Poker, Circom, and Boneh/Shoup
    Decide on circuit stack (Circom, Halo2, etc.)
    Sketch protocol phases + shuffle commitment design

Week 5: Jul 14–20

Focus: Game State & Turn Logic

    Game state replication (FSM per peer)
    Signed broadcast of actions
    Turn order + timeouts

Thesis Work:

    Card commitment + prototype shuffle ZK circuit
    Evaluate encryption strategies (ElGamal, Pedersen)

Week 6: Jul 21–27

Focus: Betting + Testing

    Betting rules (check/call/raise/fold)
    Pot tracking
    Reconnect logic

Thesis Work:

    Begin card reveal + proof generation logic
    Validate proof verification workflow across peers

Week 7: Jul 28-31

Wrap-Up:

    Final integration test with 2-3 peers
    Document project architecture and limitations
    Prepare and submit short project report (~5-10 pages)

So here's what happens after July 31:
After July 31: Thesis-Only Phase (Aug 1 - Sep 23)

You now work full-time (~40 hours/week) on the Bachelor's thesis, focused on designing, implementing, and evaluating the zero-knowledge poker protocol.

August 1-7:

    Finalize ZK circuit structure (shuffle, commitments, reveal)
    Integrate commitment verification into game logic
    Test card shuffling & reveal between 3 peers

August 8-14:

    Formalize message signing protocol for each ZK proof
    Implement failure recovery: timeouts, slashing non-cooperative peers

August 15-21:

    Optimize and benchmark circuits (proof size, proving time)
    Draft thesis chapters: Protocol Design, Implementation

August 22-31:

    Finish all code and protocol evaluation
    Start writing full Evaluation and Security chapters

September 1-14:

    Complete draft of entire thesis
    Perform code + proof review with advisor or peer
    Prepare figures, diagrams, benchmarking charts

September 15-21:

    Final thesis polishing: references, proofreading, formatting
    Create README, reproducibility guide, and demo materials

September 23:

    Submit the final thesis

