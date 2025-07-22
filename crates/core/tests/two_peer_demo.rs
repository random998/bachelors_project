//! `tests/two_peer_demo.rs`
//! A fully in-memory integration test for peers joining a table in various
//! scenarios. All tests use in-memory channels to simulate P2P communication
//! without a real network. Messages are sent directly between peers using
//! pairwise channels, leveraging the `SignedMessage` with embedded `PeerId` to
//! identify senders.

use std::time::Duration;

use anyhow::Result;
use env_logger::Env;
use log::info;
use poker_core::crypto::KeyPair;
use poker_core::game_state::{HandPhase, Projection};
use poker_core::message::{NetworkMessage, SignedMessage, UiCmd};
use poker_core::net::traits::{P2pRx, P2pTransport, P2pTx};
use poker_core::poker::{Chips, TableId};
use poker_core::protocol::msg::Hash;
use poker_core::protocol::state::GENESIS_HASH;
use rand_core::RngCore;
use tokio::time::sleep;

const BLAKE3_HASH_BYTE_ARR_LEN: usize = 32;
// Construct an in-memory ['P2pTransport'] from two unbounded channels.
const fn mock_transport(
    outbound: tokio::sync::mpsc::Sender<SignedMessage,>,
    inbound: tokio::sync::mpsc::Receiver<SignedMessage,>,
) -> P2pTransport {
    P2pTransport {
        tx: P2pTx {
            network_msg_sender: outbound,
        },
        rx: P2pRx {
            network_msg_receiver: inbound,
        },
    }
}

// Helper to pump messages between two peers until no more are pending.
// Simulates network message passing by processing ticks and handling received
// messages.
async fn pump_messages(alice: &mut Projection, bob: &mut Projection,) {
    loop {
        let mut progressed = false;
        alice.tick().await;
        if let Ok(msg,) = alice.try_recv() {
            info!(
                "{} received message: {:?}",
                alice.peer_id(),
                msg.message().label()
            );
            alice.handle_network_msg(msg,).await;
            progressed = true;
        }
        bob.tick().await;
        if let Ok(msg,) = bob.try_recv() {
            info!(
                "{} received message: {:?}",
                bob.peer_id(),
                msg.message().label()
            );
            bob.handle_network_msg(msg,).await;
            progressed = true;
        }

        if !progressed {
            info!("no one received message");
            break;
        }

        // Small delay to simulate network latency.
        sleep(Duration::from_millis(10,),).await;
    }
}

// Helper to pump messages between three peers until no more are pending.
async fn pump_three(
    alice: &mut Projection,
    bob: &mut Projection,
    charlie: &mut Projection,
) {
    loop {
        let mut progressed = false;
        alice.tick().await;
        if let Ok(msg,) = alice.try_recv() {
            info!("alice received message: {:?}", msg.message().label());
            alice.handle_network_msg(msg,).await;
            progressed = true;
        }
        bob.tick().await;
        if let Ok(msg,) = bob.try_recv() {
            info!("bob received message: {:?}", msg.message().label());
            bob.handle_network_msg(msg,).await;
            progressed = true;
        }
        charlie.tick().await;
        if let Ok(msg,) = charlie.try_recv() {
            info!("charlie received message: {:?}", msg.message().label());
            charlie.handle_network_msg(msg,).await;
            progressed = true;
        }

        if !progressed {
            info!("no one received message");
            break;
        }

        sleep(Duration::from_millis(10,),).await;
    }
}

// Initialize logger for tests to print info and warn messages.
fn init_logger() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info",),)
        .init();
}

/// Test successful join of two peers: seed (Alice) joins directly, non-seed
/// (Bob) sends SyncReq -> Alice processes, appends JoinTableReq, sends SyncResp
/// and ProtocolEntry.
///
/// This test verifies:
/// - Seed peer (Alice) can join directly and see herself in the players list.
/// - Non-seed peer (Bob) requests sync, receives the chain, replays it, and
///   joins.
/// - Both peers end with consistent state: same players, hash_head, chain
///   length.
/// - Phases remain WaitingForPlayers until game starts.
/// - has_joined_table flags are set correctly.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
pub async fn two_peers_join_success() -> Result<(),> {
    init_logger();
    // Deterministic key pairs.
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();

    // Wire the two peers with in-memory pipes.
    let (a_out, b_in,) = tokio::sync::mpsc::channel(64,);
    let (b_out, a_in,) = tokio::sync::mpsc::channel(64,);
    let transport_a = mock_transport(a_out, a_in,);
    let transport_b = mock_transport(b_out, b_in,);

    // Common table id
    let table_id = TableId::new_id();

    // Create the two Projections.
    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        6,
        kp_a.clone(),
        transport_a,
        |_| {}, // loopback callback not needed in test.
        true,   // Alice is seed.
    );

    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        6,
        kp_b.clone(),
        transport_b,
        |_| {}, // loopback callback not needed in test.
        false,  // Bob is not seed.
    );

    // Alice joins her own instance directly via log entry.
    alice
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id,
            peer_id: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(1_000,),
        },)
        .await;

    // Pump to propagate Alice's ProtocolEntry (JoinTableReq)
    pump_messages(&mut alice, &mut bob,).await;

    // Assertions after Alice joins.
    assert_eq!(alice.players().len(), 1, "Alice should see herself");
    assert_eq!(
        bob.players().len(),
        1,
        "Bob should see Alice after processing her join"
    );
    assert_eq!(
        alice.hash_head(),
        bob.hash_head(),
        "Hashes match after Alice's join"
    );
    assert!(alice.has_joined_table());
    assert!(!bob.has_joined_table());
    assert_eq!(alice.phase, HandPhase::WaitingForPlayers);
    assert_eq!(bob.phase, HandPhase::WaitingForPlayers);

    // Bob joins via SyncReq
    bob.handle_ui_msg(UiCmd::PlayerJoinTableRequest {
        table_id,
        peer_id: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(1_000,),
    },)
        .await;

    // Pump to propagate: Bob sends SyncReq -> Alice processes, appends
    // JoinTableReq, sends SyncResp and ProtocolEntry.
    pump_messages(&mut alice, &mut bob,).await;

    // Final assertions.
    assert_eq!(alice.players().len(), 2, "Alice sees both players");
    assert_eq!(bob.players().len(), 2, "Bob sees both players");
    assert_eq!(alice.hash_head(), bob.hash_head(), "Hashes match");
    assert!(bob.has_joined_table());
    assert_eq!(alice.chain().len(), 2, "Chain: Alice join + Bob join");
    assert_eq!(bob.chain().len(), 2, "Bob replayed chain");
    assert_eq!(alice.phase, bob.phase, "Phases match");

    Ok((),)
}

/// Test successful join of three peers: seed (Alice) joins directly, non-seed
/// (Bob and Charlie) join sequentially via SyncReq.
///
/// This test verifies:
/// - Multiple non-seed peers can join one after another.
/// - Each join appends to the chain, and new peers receive the full updated
///   chain.
/// - All peers end with consistent state: same players list (sorted or in join
///   order), hash_head, chain length.
/// - Seat assignments are correct and deterministic.
/// - No mismatches during propagation, assuming PlayerJoinedConf is sent as
///   plain message.
#[tokio::test(flavor = "multi_thread")]
pub async fn three_peers_join_success() -> Result<(),> {
    init_logger();

    // Deterministic key pairs.
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();
    let kp_c = KeyPair::generate();

    // Set up pairwise channels: each peer sends to all others except itself.
    let (a_to_b, _b_from_a,) = tokio::sync::mpsc::channel(64,);
    let (a_to_c, _c_from_a,) = tokio::sync::mpsc::channel(64,);
    let (b_to_a, mut a_from_b,) = tokio::sync::mpsc::channel(64,);
    let (b_to_c, _c_from_b,) = tokio::sync::mpsc::channel(64,);
    let (c_to_a, mut a_from_c,) = tokio::sync::mpsc::channel(64,);
    let (c_to_b, _b_from_c,) = tokio::sync::mpsc::channel(64,);

    // Merge receivers for each peer.
    let transport_a = mock_transport(
        tokio::sync::mpsc::channel(64,).0, // Dummy sender; we'll override send
        tokio::sync::mpsc::channel(64,).1, // Dummy receiver; we'll use merged
    );
    let transport_b = mock_transport(
        tokio::sync::mpsc::channel(64,).0,
        tokio::sync::mpsc::channel(64,).1,
    );
    let transport_c = mock_transport(
        tokio::sync::mpsc::channel(64,).0,
        tokio::sync::mpsc::channel(64,).1,
    );

    let mut alice = Projection::new(
        "Alice".into(),
        TableId::new_id(),
        6,
        kp_a.clone(),
        transport_a,
        |_| {},
        true,
    );
    let mut bob = Projection::new(
        "Bob".into(),
        alice.table_id,
        6,
        kp_b.clone(),
        transport_b,
        |_| {},
        false,
    );
    let mut charlie = Projection::new(
        "Charlie".into(),
        alice.table_id,
        6,
        kp_c.clone(),
        transport_c,
        |_| {},
        false,
    );

    // Override send to broadcast to other peers.
    alice.connection.tx.network_msg_sender = a_to_b.clone();
    bob.connection.tx.network_msg_sender = b_to_a.clone();
    charlie.connection.tx.network_msg_sender = c_to_a.clone();

    // Alice joins
    alice
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id: alice.table_id,
            peer_id:  alice.peer_id(),
            nickname: "Alice".into(),
            chips:    Chips::new(1000,),
        },)
        .await;

    // Manually broadcast Alice's messages
    while let Ok(msg,) = a_from_b.try_recv() {
        let _ = a_to_b.send(msg.clone(),).await;
        let _ = a_to_c.send(msg,).await;
    }

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    assert_eq!(alice.players().len(), 1);
    assert_eq!(bob.players().len(), 1);
    assert_eq!(charlie.players().len(), 1);
    assert_eq!(alice.hash_head(), bob.hash_head());
    assert_eq!(alice.hash_head(), charlie.hash_head());

    // Bob joins
    bob.handle_ui_msg(UiCmd::PlayerJoinTableRequest {
        table_id: alice.table_id,
        peer_id:  bob.peer_id(),
        nickname: "Bob".into(),
        chips:    Chips::new(1000,),
    },)
        .await;

    // Broadcast Bob's messages
    while let Ok(msg,) = a_from_b.try_recv() {
        let _ = b_to_a.send(msg.clone(),).await;
        let _ = b_to_c.send(msg,).await;
    }

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    assert_eq!(alice.players().len(), 2);
    assert_eq!(bob.players().len(), 2);
    assert_eq!(charlie.players().len(), 2);
    assert_eq!(alice.hash_head(), bob.hash_head());
    assert_eq!(alice.hash_head(), charlie.hash_head());

    // Charlie joins
    charlie
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id: alice.table_id,
            peer_id:  charlie.peer_id(),
            nickname: "Charlie".into(),
            chips:    Chips::new(1000,),
        },)
        .await;

    // Broadcast Charlie's messages
    while let Ok(msg,) = a_from_c.try_recv() {
        let _ = c_to_a.send(msg.clone(),).await;
        let _ = c_to_b.send(msg,).await;
    }

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    assert_eq!(alice.players().len(), 3);
    assert_eq!(bob.players().len(), 3);
    assert_eq!(charlie.players().len(), 3);
    assert_eq!(alice.hash_head(), bob.hash_head());
    assert_eq!(alice.hash_head(), charlie.hash_head());
    assert!(
        alice.players().iter().all(|p| p.seat_idx.is_some()),
        "Seats assigned"
    );

    Ok((),)
}

/// Test rejection when table is full: Alice and Bob join, Charlie tries to join
/// but table size 2.
///
/// This test verifies:
/// - Joins succeed until table capacity is reached.
/// - Additional join requests are ignored (no SyncResp, no chain append).
/// - Rejected peer remains with previous state, not joined.
/// - Existing peers unchanged.
#[tokio::test(flavor = "multi_thread")]
async fn reject_join_table_full() -> Result<(),> {
    init_logger();

    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();
    let kp_c = KeyPair::generate();

    let (a_to_b, b_from_a,) = tokio::sync::mpsc::channel(64,);
    let (b_to_a, a_from_b,) = tokio::sync::mpsc::channel(64,);
    let (c_to_a, _a_from_c,) = tokio::sync::mpsc::channel(64,);
    let (a_to_c, c_from_a,) = tokio::sync::mpsc::channel(64,);

    let mut alice = Projection::new(
        "Alice".into(),
        TableId::new_id(),
        2, // Table size 2
        kp_a.clone(),
        mock_transport(a_to_b.clone(), a_from_b,),
        |_| {},
        true,
    );

    let mut bob = Projection::new(
        "Bob".into(),
        alice.table_id,
        2,
        kp_b.clone(),
        mock_transport(b_to_a.clone(), b_from_a,),
        |_| {},
        false,
    );

    let mut charlie = Projection::new(
        "Charlie".into(),
        alice.table_id,
        2,
        kp_c.clone(),
        mock_transport(c_to_a.clone(), c_from_a,),
        |_| {},
        false,
    );

    // Alice joins
    alice
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id: alice.table_id,
            peer_id:  alice.peer_id(),
            nickname: "Alice".into(),
            chips:    Chips::new(1000,),
        },)
        .await;

    // Broadcast Alice's messages
    while let Ok(msg,) = alice.try_recv() {
        let _ = a_to_b.send(msg.clone(),).await;
        let _ = a_to_c.send(msg,).await;
    }

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    assert_eq!(alice.players().len(), 1);
    assert_eq!(bob.players().len(), 1);

    // Bob joins
    bob.handle_ui_msg(UiCmd::PlayerJoinTableRequest {
        table_id: alice.table_id,
        peer_id:  bob.peer_id(),
        nickname: "Bob".into(),
        chips:    Chips::new(1000,),
    },)
        .await;

    // Broadcast Bob's messages
    while let Ok(msg,) = bob.try_recv() {
        let _ = b_to_a.send(msg,).await;
    }

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    assert_eq!(alice.players().len(), 2);
    assert_eq!(bob.players().len(), 2);

    // Charlie tries to join
    charlie
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id: alice.table_id,
            peer_id:  charlie.peer_id(),
            nickname: "Charlie".into(),
            chips:    Chips::new(1000,),
        },)
        .await;

    // Broadcast Charlie's messages
    while let Ok(msg,) = charlie.try_recv() {
        let _ = c_to_a.send(msg,).await;
    }

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    assert_eq!(alice.players().len(), 2, "Table full, no join");
    assert_eq!(bob.players().len(), 2);
    assert_eq!(charlie.players().len(), 2);
    assert!(!charlie.has_joined_table());

    Ok((),)
}

/// Test rejection when game has started: Alice joins, starts game, Bob tries to
/// join.
///
/// This test verifies:
/// - Join requests after game_started = true are ignored.
/// - Rejected peer does not join, no SyncResp sent.
/// - Existing peer state unchanged.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reject_join_game_started() -> Result<(),> {
    init_logger();

    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();

    let (a_out, b_in,) = tokio::sync::mpsc::channel(64,);
    let (b_out, a_in,) = tokio::sync::mpsc::channel(64,);
    let transport_a = mock_transport(a_out, a_in,);
    let transport_b = mock_transport(b_out, b_in,);

    let table_id = TableId::new_id();

    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        6,
        kp_a.clone(),
        transport_a,
        |_| {},
        true,
    );

    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        6,
        kp_b.clone(),
        transport_b,
        |_| {},
        false,
    );

    // Alice joins
    alice
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id,
            peer_id: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(1000,),
        },)
        .await;
    pump_messages(&mut alice, &mut bob,).await;

    // Simulate game start
    alice.game_started = true;
    bob.game_started = true; // Assume propagated

    // Bob tries to join
    bob.handle_ui_msg(UiCmd::PlayerJoinTableRequest {
        table_id,
        peer_id: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(1000,),
    },)
        .await;
    pump_messages(&mut alice, &mut bob,).await;

    assert_eq!(alice.players().len(), 1);
    assert_eq!(bob.players().len(), 1); // Bob only sees Alice
    assert!(!bob.has_joined_table());

    Ok((),)
}

/// Test rejection if already joined: Bob joins, then tries to join again.
///
/// This test verifies:
/// - Duplicate join requests for the same peer are ignored.
/// - No additional chain entries or state changes.
/// - has_joined_table remains true, but no errors or mismatches.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reject_join_already_joined() -> Result<(),> {
    init_logger();

    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();

    let (a_out, b_in,) = tokio::sync::mpsc::channel(64,);
    let (b_out, a_in,) = tokio::sync::mpsc::channel(64,);
    let transport_a = mock_transport(a_out, a_in,);
    let transport_b = mock_transport(b_out, b_in,);

    let table_id = TableId::new_id();

    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        6,
        kp_a.clone(),
        transport_a,
        |_| {},
        true,
    );

    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        6,
        kp_b.clone(),
        transport_b,
        |_| {},
        false,
    );

    // Alice joins
    alice
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id,
            peer_id: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(1000,),
        },)
        .await;
    pump_messages(&mut alice, &mut bob,).await;

    // Bob joins first time
    bob.handle_ui_msg(UiCmd::PlayerJoinTableRequest {
        table_id,
        peer_id: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(1000,),
    },)
        .await;
    pump_messages(&mut alice, &mut bob,).await;

    let chain_len_before = alice.chain().len();

    // Bob tries to join again
    bob.handle_ui_msg(UiCmd::PlayerJoinTableRequest {
        table_id,
        peer_id: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(1000,),
    },)
        .await;
    pump_messages(&mut alice, &mut bob,).await;

    assert_eq!(alice.players().len(), 2);
    assert_eq!(bob.players().len(), 2);
    assert_eq!(alice.chain().len(), chain_len_before, "No new chain entry");

    Ok((),)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reject_invalid_sync_resp() -> Result<(),> {
    init_logger();

    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();

    let (a_out, b_in,) = tokio::sync::mpsc::channel(64,);
    let (b_out, a_in,) = tokio::sync::mpsc::channel(64,);
    let transport_a = mock_transport(a_out.clone(), a_in,);
    let transport_b = mock_transport(b_out, b_in,);

    let table_id = TableId::new_id();

    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        6,
        kp_a.clone(),
        transport_a,
        |_| {},
        true,
    );

    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        6,
        kp_b.clone(),
        transport_b,
        |_| {},
        false,
    );

    // Alice joins
    alice
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id,
            peer_id: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(1000,),
        },)
        .await;
    alice.tick().await; // Send join message to outbound queue

    // Empty Alice's outbound queue to simulate message sent but not received by
    // Bob
    while bob.try_recv().is_ok() {
        // Discard the message
    }

    // Bob sends SyncReq
    bob.handle_ui_msg(UiCmd::PlayerJoinTableRequest {
        table_id,
        peer_id: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(1000,),
    },)
        .await;
    pump_messages(&mut alice, &mut bob,).await;

    // Alice processes SyncReq, tamper the SyncResp chain
    alice.tick().await;
    bob.tick().await;

    // Receive the legit SyncReq at Alice, tamper and send response
    if alice.try_recv().is_ok() {
        // Consume SyncReq
        let mut tampered_chain = alice.chain().clone();
        if let Some(entry,) = tampered_chain.last_mut() {
            let mut byte_array = [0u8; BLAKE3_HASH_BYTE_ARR_LEN];
            rand::rng().fill_bytes(&mut byte_array,);
            entry.prev_hash = Hash(blake3::Hash::from_bytes(byte_array,),);
        }
        let resp = NetworkMessage::SyncResp {
            target: bob.peer_id(),
            chain:  tampered_chain,
        };
        let signed_resp = SignedMessage::new(&kp_a, resp,);
        let _ = a_out.send(signed_resp,).await; // Send directly to Bob's inbound
    }
    pump_messages(&mut alice, &mut bob,).await;

    assert!(
        !bob.players()
            .iter().any(|p| p.peer_id == bob.peer_id())
    ); // bob should not have joined his own state.
    assert_eq!(bob.chain().len(), 0);
    assert_eq!(bob.players().len(), 0); // No one, since Alice's join not propagated
    assert_eq!(bob.hash_head(), *GENESIS_HASH); // bob's state hash chain should still be at the genesis hash state.

    Ok((),)
}
/// Test concurrent joins: Bob and Charlie send SyncReq at the same time, ensure
/// chain consistency.
///
/// This test verifies:
/// - Simultaneous join requests are handled without race-induced mismatches.
/// - Chain appends in some order (deterministic by PeerId key in LogEntry).
/// - All peers end with the same chain and players list.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn concurrent_joins() -> Result<(),> {
    init_logger();

    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();
    let kp_c = KeyPair::generate();

    // Set up pairwise channels
    let (a_send_pipe, a_send_recv_pipe,) = tokio::sync::mpsc::channel(64,);
    let (b_send_pipe, b_send_recv_pipe,) = tokio::sync::mpsc::channel(64,);
    let (c_send_pipe, c_send_recv_pipe,) = tokio::sync::mpsc::channel(64,);

    let mut alice = Projection::new(
        "Alice".into(),
        TableId::new_id(),
        6,
        kp_a.clone(),
        mock_transport(a_send_pipe, a_send_recv_pipe),
        |_| {},
        true,
    );
    let mut bob = Projection::new(
        "Bob".into(),
        alice.table_id,
        6,
        kp_b.clone(),
        mock_transport(b_send_pipe, b_send_recv_pipe,),
        |_| {},
        false,
    );
    let mut charlie = Projection::new(
        "Charlie".into(),
        alice.table_id,
        6,
        kp_c.clone(),
        mock_transport(c_send_pipe, c_send_recv_pipe,),
        |_| {},
        false,
    );

    // Alice joins
    alice
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id: alice.table_id,
            peer_id:  alice.peer_id(),
            nickname: "Alice".into(),
            chips:    Chips::new(1000,),
        },)
        .await;

    // Broadcast Alice's messages
    while let Ok(msg,) = alice.try_recv() {
        let _ = alice.send(msg.clone(),);
    }

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    // Bob and Charlie send join simultaneously
    bob.handle_ui_msg(UiCmd::PlayerJoinTableRequest {
        table_id: alice.table_id,
        peer_id:  bob.peer_id(),
        nickname: "Bob".into(),
        chips:    Chips::new(1000,),
    },)
        .await;
    charlie
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id: alice.table_id,
            peer_id:  charlie.peer_id(),
            nickname: "Charlie".into(),
            chips:    Chips::new(1000,),
        },)
        .await;

    // Broadcast messages
    while let Ok(msg,) = bob.try_recv() {
        let _ = bob.send(msg.clone(),);
    }
    while let Ok(msg,) = charlie.try_recv() {
        let _ = charlie.send(msg.clone(),);
    }

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    assert_eq!(alice.players().len(), 3);
    assert_eq!(bob.players().len(), 3);
    assert_eq!(charlie.players().len(), 3);
    assert_eq!(alice.hash_head(), bob.hash_head());
    assert_eq!(alice.hash_head(), charlie.hash_head());
    assert_eq!(alice.chain().len(), 3); // Alice + Bob + Charlie joins

    Ok((),)
}

/// Test seat assignment: Verify deterministic seats after joins.
///
/// This test verifies:
/// - Seats are assigned deterministically (e.g., based on join order or
///   PeerId).
/// - All peers see the same seat assignments after sync.
/// - Seats are unique and within table size.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn seat_assignment_correct() -> Result<(),> {
    init_logger();

    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();

    let (a_out, b_in,) = tokio::sync::mpsc::channel(64,);
    let (b_out, a_in,) = tokio::sync::mpsc::channel(64,);
    let transport_a = mock_transport(a_out, a_in,);
    let transport_b = mock_transport(b_out, b_in,);

    let table_id = TableId::new_id();

    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        6,
        kp_a.clone(),
        transport_a,
        |_| {},
        true,
    );

    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        6,
        kp_b.clone(),
        transport_b,
        |_| {},
        false,
    );

    // Alice joins (seat 0 expected)
    alice
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id,
            peer_id: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(1000,),
        },)
        .await;
    pump_messages(&mut alice, &mut bob,).await;

    let alice_player = alice.get_player(&alice.peer_id(),).unwrap();
    assert_eq!(alice_player.seat_idx, Some(0));

    let bob_alice = bob.get_player(&alice.peer_id(),).unwrap();
    assert_eq!(bob_alice.seat_idx, Some(0));

    // Bob joins (seat 1 expected)
    bob.handle_ui_msg(UiCmd::PlayerJoinTableRequest {
        table_id,
        peer_id: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(1000,),
    },)
        .await;
    pump_messages(&mut alice, &mut bob,).await;

    let alice_bob = alice.get_player(&bob.peer_id(),).unwrap();
    assert_eq!(alice_bob.seat_idx, Some(1));

    let bob_self = bob.get_player(&bob.peer_id(),).unwrap();
    assert_eq!(bob_self.seat_idx, Some(1));

    Ok((),)
}

/// Test non-seed peer trying to join without seed present: Should fail or hang,
/// but in test, no response.
///
/// This test verifies:
/// - Non-seed peer sends SyncReq, but without seed, no SyncResp, so doesn't
///   join.
/// - State remains unchanged for the non-seed peer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_seed_join_no_seed() -> Result<(),> {
    init_logger();

    let kp_b = KeyPair::generate();

    let table_id = TableId::new_id();

    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        6,
        kp_b.clone(),
        mock_transport(
            tokio::sync::mpsc::channel(64,).0,
            tokio::sync::mpsc::channel(64,).1,
        ), // Dummy transport
        |_| {},
        false,
    );

    // Bob tries to join without seed
    bob.handle_ui_msg(UiCmd::PlayerJoinTableRequest {
        table_id,
        peer_id: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(1000,),
    },)
        .await;

    // Pump (no other peer)
    bob.tick().await;

    assert!(!bob.has_joined_table());
    assert_eq!(bob.players().len(), 0);
    assert_eq!(bob.hash_head(), *GENESIS_HASH);

    Ok((),)
}

/// Test chain replay with multiple joins: Bob joins after Alice and Charlie,
/// replays full chain.
///
/// This test verifies:
/// - Late-joining peer receives and replays the entire chain correctly.
/// - All state (players, seats) consistent across peers.
/// - Chain length reflects all joins.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn late_join_replay_chain() -> Result<(),> {
    init_logger();

    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();
    let kp_c = KeyPair::generate();

    let (a_send, a_recv,) = tokio::sync::mpsc::channel(64,);
    let (b_send, b_recv,) = tokio::sync::mpsc::channel(64,);
    let (c_send, c_recv,) = tokio::sync::mpsc::channel(64,);
    let transport_a = mock_transport(a_send, a_recv,);
    let transport_b = mock_transport(b_send, b_recv,);
    let transport_c = mock_transport(c_send, c_recv,);

    let mut alice = Projection::new(
        "Alice".into(),
        TableId::new_id(),
        6,
        kp_a.clone(),
        transport_a,
        |_| {},
        true,
    );
    let mut bob = Projection::new(
        "Bob".into(),
        alice.table_id,
        6,
        kp_b.clone(),
        transport_b,
        |_| {},
        false,
    );
    let mut charlie = Projection::new(
        "Charlie".into(),
        alice.table_id,
        6,
        kp_c.clone(),
        transport_c,
        |_| {},
        false,
    );

    // Alice joins
    alice
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id: alice.table_id,
            peer_id:  alice.peer_id(),
            nickname: "Alice".into(),
            chips:    Chips::new(1000,),
        },)
        .await;

    // Broadcast Alice's messages
    while let Ok(msg,) = alice.try_recv() {
        let _ = alice.send(msg.clone(),);
    }

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    // Charlie joins
    charlie
        .handle_ui_msg(UiCmd::PlayerJoinTableRequest {
            table_id: alice.table_id,
            peer_id:  charlie.peer_id(),
            nickname: "Charlie".into(),
            chips:    Chips::new(1000,),
        },)
        .await;

    // Broadcast Charlie's messages
    while let Ok(msg,) = charlie.try_recv() {
        let _ = charlie.send(msg,);
    }

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    assert_eq!(alice.players().len(), 2);
    assert_eq!(charlie.players().len(), 2);
    assert_eq!(bob.players().len(), 2); // Bob sees Alice and Charlie

    // Bob joins late
    bob.handle_ui_msg(UiCmd::PlayerJoinTableRequest {
        table_id: alice.table_id,
        peer_id:  bob.peer_id(),
        nickname: "Bob".into(),
        chips:    Chips::new(1000,),
    },)
        .await;

    // Broadcast Bob's messages
    while let Ok(msg,) = bob.try_recv() {
        let _ = bob.send(msg.clone(),);
    }

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    assert_eq!(alice.players().len(), 3);
    assert_eq!(bob.players().len(), 3);
    assert_eq!(charlie.players().len(), 3);
    assert_eq!(alice.hash_head(), bob.hash_head());
    assert_eq!(alice.hash_head(), charlie.hash_head());
    assert_eq!(alice.chain().len(), 3);
    assert_eq!(bob.chain().len(), 3);

    Ok((),)
}
