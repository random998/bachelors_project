//! `crates/integration-tests/tests/two_peer_demo.rs`
//! Integration tests for peers joining a table using the same libp2p swarm
//! architecture as defined in `p2p-net::swarm_task`. Tests use Gossipsub with
//! `MemoryTransport` for in-memory testing to simulate network communication.

mod support;

use std::time::Duration;

use anyhow::Result;
use env_logger::Env;
use log::{info, warn};
use p2p_net::swarm_task;
use poker_core::crypto::KeyPair;
use poker_core::game_state::{HandPhase, Projection};
use poker_core::message::{NetworkMessage, SignedMessage, UIEvent};
use poker_core::poker::{Chips, TableId};
use poker_core::protocol::msg::Hash;
use poker_core::protocol::state::GENESIS_HASH;
use rand::{RngCore, thread_rng};
use tokio::time::sleep;

use crate::support::mock_gui::MockUi;

const BLAKE3_HASH_BYTE_ARR_LEN: usize = 32;
const MESSAGE_RECEIVE_TIMEOUT: u64 = 5;
const NETWORK_PUMP_MS_DELAY: u64 = 10;
const CHIPS_JOIN_AMOUNT: u32 = 1_000;

// Initialize logger for tests to print info and warn messages.
fn init_logger() {
    let _ = env_logger::Builder::from_env(
        Env::default().default_filter_or("error",),
    )
    .try_init();
}

async fn wait_for_listen_addr(proj: &mut Projection,) {
    let mut attempts = 0;
    loop {
        proj.tick().await;
        while let Ok(msg,) = proj.try_recv() {
            proj.handle_network_msg(msg,).await;
        }
        if proj.listen_addr.is_some() {
            break;
        }
        sleep(Duration::from_millis(50,),).await;
        attempts += 1;
        assert!(attempts <= 20, "Timeout waiting for listen addr");
    }
}

// Helper to pump messages by polling until idle, ensuring listen addresses are
// set.
async fn pump_three(
    alice: &mut Projection,
    bob: &mut Projection,
    charlie: &mut Projection,
) {
    let timeout = Duration::from_secs(MESSAGE_RECEIVE_TIMEOUT,); // Prevent infinite loop
    let start = tokio::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            warn!("pump_three timed out after {timeout:?}");
            break;
        }

        alice.tick().await;
        while let Ok(msg,) = alice.try_recv() {
            info!("alice received message: {}", msg.message());
            alice.handle_network_msg(msg,).await;
        }

        bob.tick().await;
        while let Ok(msg,) = bob.try_recv() {
            info!("bob received message: {}", msg.message());
            bob.handle_network_msg(msg,).await;
        }

        charlie.tick().await;
        while let Ok(msg,) = charlie.try_recv() {
            info!("charlie received message: {}", msg.message());
            charlie.handle_network_msg(msg,).await;
        }

        sleep(Duration::from_millis(NETWORK_PUMP_MS_DELAY,),).await;
    }
}

// Helper for two peers, used in tests with only Alice and Bob
async fn pump_messages(alice: &mut Projection, bob: &mut Projection,) {
    let timeout = Duration::from_secs(5,);
    let start = tokio::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            warn!("pump_messages timed out after {timeout:?}");
            break;
        }

        alice.tick().await;
        while let Ok(msg,) = alice.try_recv() {
            info!("alice received message: {}", msg.message());
            alice.handle_network_msg(msg,).await;
            alice.update().await;
        }

        bob.tick().await;
        while let Ok(msg,) = bob.try_recv() {
            info!("bob received message: {}", msg.message());
            bob.handle_network_msg(msg,).await;
            bob.update().await;
        }

        sleep(Duration::from_millis(NETWORK_PUMP_MS_DELAY,),).await;
    }
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
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_peers_join_success() -> Result<(),> {
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();
    let table_id = TableId::new_id();

    // Alice as seed
    let transport_a = swarm_task::new(&table_id, kp_a.clone(), None,);
    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        6,
        kp_a.clone(),
        transport_a,
        true,
    );

    wait_for_listen_addr(&mut alice,).await;
    let alice_addr = alice.listen_addr.clone().expect("Alice listen addr",);

    // Bob dials Alice
    let transport_b =
        swarm_task::new(&table_id, kp_b.clone(), Some(alice_addr,),);
    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        6,
        kp_b.clone(),
        transport_b,
        false,
    );

    wait_for_listen_addr(&mut bob,).await;

    // Alice joins
    alice
        .handle_ui_msg(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(1_000,),
        },)
        .await;

    pump_messages(&mut alice, &mut bob,).await;

    // Assertions after Alice joins
    assert_eq!(alice.players().len(), 1, "Alice should see herself");
    assert_eq!(
        bob.players().len(),
        0,
        "Bob should see Alice after processing her join, but reject, since he has not synced yet"
    );
    assert!(alice.has_joined_table());
    assert!(!bob.has_joined_table());
    assert_eq!(alice.phase(), HandPhase::WaitingForPlayers);
    assert_eq!(bob.phase(), HandPhase::WaitingForPlayers);

    // Bob joins via SyncReq
    bob.handle_ui_msg(UIEvent::PlayerJoinTableRequest {
        table_id,
        player_requesting_join: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(1_000,),
    },)
        .await;

    pump_messages(&mut alice, &mut bob,).await;

    // Final assertions
    assert_eq!(alice.players().len(), 2, "Alice sees both players");
    assert_eq!(bob.players().len(), 2, "Bob sees both players");
    assert_eq!(alice.hash_head(), bob.hash_head(), "Hashes match");
    assert!(bob.has_joined_table());
    assert_eq!(alice.hash_chain().len(), 2, "Chain: Alice join + Bob join");
    assert_eq!(bob.hash_chain().len(), 2, "Bob replayed chain");
    assert_eq!(alice.phase(), bob.phase(), "Phases match");

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
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn three_peers_join_success() -> Result<(),> {
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();
    let kp_c = KeyPair::generate();
    let table_id = TableId::new_id();

    // Alice as seed
    let transport_a = swarm_task::new(&table_id, kp_a.clone(), None,);
    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        6,
        kp_a.clone(),
        transport_a,
        true,
    );

    // Wait for Alice's listen address
    wait_for_listen_addr(&mut alice,).await;
    let alice_addr = alice.listen_addr.clone().expect("Alice listen addr",);

    // Bob dials Alice
    let transport_b =
        swarm_task::new(&table_id, kp_b.clone(), Some(alice_addr.clone(),),);
    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        6,
        kp_b.clone(),
        transport_b,
        false,
    );
    bob.tick().await;
    wait_for_listen_addr(&mut bob,).await;

    // Charlie dials Alice
    let transport_c =
        swarm_task::new(&table_id, kp_c.clone(), Some(alice_addr,),);
    let mut charlie = Projection::new(
        "Charlie".into(),
        table_id,
        6,
        kp_c.clone(),
        transport_c,
        false,
    );
    charlie.tick().await;
    wait_for_listen_addr(&mut charlie,).await;

    // Alice joins
    alice
        .handle_ui_msg(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await;

    pump_three(&mut alice, &mut bob, &mut charlie,).await;
    alice.tick().await;
    bob.tick().await;
    charlie.tick().await;

    assert_eq!(alice.players().len(), 1);
    assert_eq!(charlie.players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
    assert_eq!(bob.players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
    assert_eq!(bob.hash_head(), charlie.hash_head());

    // Bob joins
    bob.handle_ui_msg(UIEvent::PlayerJoinTableRequest {
        table_id,
        player_requesting_join: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(CHIPS_JOIN_AMOUNT,),
    },)
        .await;

    pump_three(&mut alice, &mut bob, &mut charlie,).await;
    alice.tick().await;
    bob.tick().await;
    charlie.tick().await;

    assert_eq!(alice.players().len(), 2);
    assert_eq!(bob.players().len(), 2);
    assert_eq!(charlie.players().len(), 0); // charlie has 0 players, since he has not synced yet.
    assert_eq!(alice.hash_chain().len(), 2);
    assert_eq!(bob.hash_chain().len(), 2);
    assert_eq!(charlie.hash_chain().len(), 0); // charlie has not advanced his hash chain, since he has not synced yet.
    assert_eq!(alice.hash_head(), bob.hash_head());

    // Charlie joins
    charlie
        .handle_ui_msg(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: charlie.peer_id(),
            nickname: "Charlie".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await;

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    assert_eq!(alice.players().len(), 3);
    assert_eq!(bob.players().len(), 3);
    assert_eq!(charlie.players().len(), 3);
    assert_eq!(alice.hash_chain().len(), 3);
    assert_eq!(bob.hash_chain().len(), 3);
    assert_eq!(charlie.players().len(), 3);
    assert_eq!(alice.hash_head(), bob.hash_head());
    assert_eq!(alice.hash_head(), charlie.hash_head());
    assert_eq!(bob.hash_head(), charlie.hash_head());
    assert_eq!(alice.hash_chain(), bob.hash_chain());
    assert_eq!(alice.hash_head(), charlie.hash_head());
    assert_eq!(bob.hash_head(), charlie.hash_head());

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
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn reject_join_table_full() -> Result<(),> {
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();
    let kp_c = KeyPair::generate();

    let table_id = TableId::new_id();

    // Alice as seed
    let transport_a = swarm_task::new(&table_id, kp_a.clone(), None,);
    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        2, // Table size 2
        kp_a.clone(),
        transport_a,
        true,
    );

    wait_for_listen_addr(&mut alice,).await;
    let alice_addr = alice.listen_addr.clone().expect("Alice listen addr",);

    // Bob dials Alice
    let transport_b =
        swarm_task::new(&table_id, kp_b.clone(), Some(alice_addr.clone(),),);
    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        2,
        kp_b.clone(),
        transport_b,
        false,
    );

    // Charlie dials Alice
    let transport_c =
        swarm_task::new(&table_id, kp_c.clone(), Some(alice_addr,),);
    let mut charlie = Projection::new(
        "Charlie".into(),
        table_id,
        2,
        kp_c.clone(),
        transport_c,
        false,
    );

    // Wait for Bob and Charlie's listen addresses
    wait_for_listen_addr(&mut bob,).await;
    wait_for_listen_addr(&mut charlie,).await;

    // Alice joins
    alice
        .handle_ui_msg(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await;

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    assert_eq!(alice.players().len(), 1);
    assert_eq!(bob.players().len(), 0); // bob has not added alice to his player's list, because he has not synced yet.
    assert_eq!(charlie.players().len(), 0); // charlie has not added alice to his player's list, because he has not synced yet.

    // Bob joins
    bob.handle_ui_msg(UIEvent::PlayerJoinTableRequest {
        table_id,
        player_requesting_join: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(CHIPS_JOIN_AMOUNT,),
    },)
        .await;

    // send request from bob to alice.
    pump_three(&mut alice, &mut bob, &mut charlie,).await;
    // tick all three players such that they can process the messages.
    alice.tick().await;
    bob.tick().await;
    charlie.tick().await;
    // send response from alice to bob.
    pump_three(&mut alice, &mut bob, &mut charlie,).await;
    // tick all three players such that they can process the messages.
    alice.tick().await;
    bob.tick().await;
    charlie.tick().await;

    assert_eq!(alice.players().len(), 2);
    assert_eq!(bob.players().len(), 2);
    assert_eq!(charlie.players().len(), 0); // charlie has 0 players in his list, because he has not synced, yet.
    assert_eq!(alice.hash_chain().len(), 2);
    assert_eq!(bob.hash_chain().len(), 2);
    assert_eq!(charlie.hash_chain().len(), 0); // charlies hash chain should have length 0, because he has not synced, yet.
    assert_eq!(alice.hash_chain(), bob.hash_chain());

    // Charlie tries to join
    charlie
        .handle_ui_msg(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: charlie.peer_id(),
            nickname: "Charlie".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await;

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    assert_eq!(
        alice.players().len(),
        2,
        "table is full, charlie has not joined"
    );
    assert_eq!(bob.players().len(), 2, "table is full, charlie has not joined");
    assert_eq!(
        alice.hash_chain().len(),
        2,
        "table is full, charlie has not joined"
    );
    assert_eq!(
        bob.hash_chain().len(),
        2,
        "table is full, charlie has not joined"
    );
    assert!(
        !charlie.has_joined_table(),
        "table is full, charlie has not joined"
    );
    assert_eq!(alice.hash_chain(), bob.hash_chain());
    assert_eq!(bob.hash_chain(), alice.hash_chain());
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
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();

    let table_id = TableId::new_id();

    // Alice as seed
    let transport_a = swarm_task::new(&table_id, kp_a.clone(), None,);
    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        6,
        kp_a.clone(),
        transport_a,
        true,
    );

    wait_for_listen_addr(&mut alice,).await;
    let alice_addr = alice.listen_addr.clone().expect("Alice listen addr",);

    // Bob dials Alice
    let transport_b =
        swarm_task::new(&table_id, kp_b.clone(), Some(alice_addr,),);
    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        6,
        kp_b.clone(),
        transport_b,
        false,
    );

    wait_for_listen_addr(&mut bob,).await;

    // Alice joins
    alice
        .handle_ui_msg(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await;
    pump_messages(&mut alice, &mut bob,).await;

    // Simulate game start
    alice.game_started = true;
    bob.game_started = true; // Assume propagated

    // Bob tries to join
    bob.handle_ui_msg(UIEvent::PlayerJoinTableRequest {
        table_id,
        player_requesting_join: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(CHIPS_JOIN_AMOUNT,),
    },)
        .await;
    pump_messages(&mut alice, &mut bob,).await;

    assert_eq!(alice.players().len(), 1);
    assert_eq!(bob.players().len(), 0); // bob has not added alice, since he has not synced yet and therefore rejects any log entries that he receives.
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
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();

    let table_id = TableId::new_id();

    // Alice as seed
    let transport_a = swarm_task::new(&table_id, kp_a.clone(), None,);
    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        6,
        kp_a.clone(),
        transport_a,
        true,
    );

    wait_for_listen_addr(&mut alice,).await;
    alice.tick().await;
    let alice_addr = alice.listen_addr.clone().expect("Alice listen addr",);

    // Bob dials Alice
    let transport_b =
        swarm_task::new(&table_id, kp_b.clone(), Some(alice_addr,),);
    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        6,
        kp_b.clone(),
        transport_b,
        false,
    );
    wait_for_listen_addr(&mut bob,).await;
    alice.tick().await;
    bob.tick().await;

    // Alice joins
    alice
        .handle_ui_msg(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await;
    pump_messages(&mut alice, &mut bob,).await;
    alice.tick().await;
    bob.tick().await;

    assert_eq!(alice.players().len(), 1); // expect that alice joined her own instance.
    assert_eq!(bob.players().len(), 0); // expect that alice has not joined bobs instance, because he has not synced yet.
    assert!(!bob.has_joined_table()); // expect that bob has not joined a table, yet.
    assert_ne!(alice.hash_chain(), bob.hash_chain()); // expect that the chains diverge, because bob has not sent a SyncRequest, yet.

    // Bob joins first time
    bob.handle_ui_msg(UIEvent::PlayerJoinTableRequest {
        table_id,
        player_requesting_join: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(CHIPS_JOIN_AMOUNT,),
    },)
        .await;
    pump_messages(&mut bob, &mut alice,).await;
    bob.tick().await;
    alice.tick().await;

    let chain_len_before = alice.hash_chain().len();

    // Bob tries to join again
    bob.handle_ui_msg(UIEvent::PlayerJoinTableRequest {
        table_id,
        player_requesting_join: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(CHIPS_JOIN_AMOUNT,),
    },)
        .await;
    pump_messages(&mut alice, &mut bob,).await;

    assert_eq!(alice.players().len(), 2);
    assert_eq!(bob.players().len(), 2);
    assert_eq!(
        alice.hash_chain().len(),
        chain_len_before,
        "No new chain entry"
    );
    assert_eq!(bob.hash_chain(), alice.hash_chain());

    Ok((),)
}

/// Test rejection of invalid SyncResp: Alice sends tampered chain to Bob, Bob
/// rejects.
///
/// This test verifies:
/// - Bob discards invalid chain (tampered hash) and does not join.
/// - Bob's state remains at genesis (no change).
/// - Alice's state unchanged.
/// - Warn log for mismatch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reject_invalid_sync_resp() -> Result<(),> {
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();

    let table_id = TableId::new_id();

    // Alice as seed
    let transport_a = swarm_task::new(&table_id, kp_a.clone(), None,);
    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        6,
        kp_a.clone(),
        transport_a,
        true,
    );

    wait_for_listen_addr(&mut alice,).await;
    let alice_addr = alice.listen_addr.clone().expect("Alice listen addr",);

    // Bob dials Alice
    let transport_b =
        swarm_task::new(&table_id, kp_b.clone(), Some(alice_addr,),);
    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        6,
        kp_b.clone(),
        transport_b,
        false,
    );

    wait_for_listen_addr(&mut bob,).await;

    // Alice joins
    alice
        .handle_ui_msg(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await;
    alice.tick().await;

    // Discard messages at Bob to simulate not receiving Alice's join
    // ProtocolEntry
    while bob.try_recv().is_ok() {
        // Discard without handling
    }

    // Bob sends SyncReq
    bob.handle_ui_msg(UIEvent::PlayerJoinTableRequest {
        table_id,
        player_requesting_join: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(CHIPS_JOIN_AMOUNT,),
    },)
        .await;

    alice.tick().await;
    bob.tick().await;
    sleep(Duration::from_millis(100,),).await; // Allow message propagation

    // Consume the SyncReq at Alice without handling it
    if alice.try_recv().is_ok() {
        // Consumed SyncReq, now tamper the current chain (only Alice's join)
        let mut tampered_chain = alice.hash_chain();
        if let Some(entry,) = tampered_chain.last_mut() {
            let mut byte_array = [0u8; BLAKE3_HASH_BYTE_ARR_LEN];
            thread_rng().fill_bytes(&mut byte_array,);
            entry.prev_hash = Hash(blake3::Hash::from_bytes(byte_array,),);
        }
        let resp = NetworkMessage::SyncResp {
            player_asking_for_sync: bob.peer_id(),
            chain:                  tampered_chain,
        };
        let signed_resp = SignedMessage::new(&kp_a, resp,);
        let _ = alice.send(signed_resp,); // Send tampered response
    }

    pump_messages(&mut alice, &mut bob,).await;

    assert!(!bob.players().iter().any(|p| p.peer_id == bob.peer_id())); // bob should not have joined his own state.
    assert_eq!(bob.hash_chain().len(), 0);
    assert_eq!(bob.players().len(), 0); // No one, since Alice's join not propagated
    assert_eq!(bob.hash_head(), *GENESIS_HASH); // bobs state hash chain should still be at the genesis hash state.

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

    let table_id = TableId::new_id();

    // Alice as seed
    let transport_a = swarm_task::new(&table_id, kp_a.clone(), None,);
    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        6,
        kp_a.clone(),
        transport_a,
        true,
    );

    // Wait for Alice's listen address
    wait_for_listen_addr(&mut alice,).await;
    let alice_addr = alice.listen_addr.clone().expect("Alice listen addr",);

    // Charlie dials Alice
    let transport_c =
        swarm_task::new(&table_id, kp_c.clone(), Some(alice_addr.clone(),),);
    let mut charlie = Projection::new(
        "Charlie".into(),
        table_id,
        6,
        kp_c.clone(),
        transport_c,
        false,
    );

    wait_for_listen_addr(&mut charlie,).await;

    // Establish connection between Alice and Charlie
    pump_messages(&mut alice, &mut charlie,).await;

    // Alice joins
    alice
        .handle_ui_msg(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(1000,),
        },)
        .await;
    pump_messages(&mut alice, &mut charlie,).await; // Pump for Alice and Charlie only

    // Charlie joins
    charlie
        .handle_ui_msg(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: charlie.peer_id(),
            nickname: "Charlie".into(),
            chips: Chips::new(1000,),
        },)
        .await;

    pump_messages(&mut alice, &mut charlie,).await; // charlie sends SyncRequest to alice.
    // alice creates SyncResponse
    alice.tick().await;
    charlie.tick().await;
    pump_messages(&mut alice, &mut charlie,).await; // alice sends SyncResponse to charlie.
    // charlie processes SyncResponse
    alice.tick().await;
    charlie.tick().await;

    assert_eq!(alice.players().len(), 2);
    assert_eq!(charlie.players().len(), 2);

    // Now create Bob late
    let transport_b =
        swarm_task::new(&table_id, kp_b.clone(), Some(alice_addr,),);
    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        6,
        kp_b.clone(),
        transport_b,
        false,
    );
    wait_for_listen_addr(&mut bob,).await;

    // Establish connection for Bob with existing peers
    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    // Bob joins late, should send SyncReq and replay full chain
    bob.handle_ui_msg(UIEvent::PlayerJoinTableRequest {
        table_id,
        player_requesting_join: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(1000,),
    },)
        .await;

    // request sent from bob to peers
    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    // give time to players to apply changes introduced by messages.
    alice.tick().await;
    bob.tick().await;
    charlie.tick().await;

    // replies sent from peers to bob
    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    // give time to players to apply changes introduced by messages.
    alice.tick().await;
    bob.tick().await;
    charlie.tick().await;

    assert_eq!(alice.players().len(), 3);
    assert_eq!(bob.players().len(), 3);
    assert_eq!(charlie.players().len(), 3);
    assert_eq!(alice.hash_head(), bob.hash_head());
    assert_eq!(alice.hash_head(), charlie.hash_head());
    assert_eq!(alice.hash_chain().len(), 3);
    assert_eq!(bob.hash_chain().len(), 3);
    assert_eq!(alice.hash_chain(), bob.hash_chain());
    assert_eq!(alice.hash_chain(), charlie.hash_chain());
    assert_eq!(bob.hash_chain(), charlie.hash_chain());

    Ok((),)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn game_starts_correctly() -> Result<(),> {
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();
    let kp_c = KeyPair::generate();

    let table_id = TableId::new_id();

    // Alice as seed
    let transport_a = swarm_task::new(&table_id, kp_a.clone(), None,);
    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        3,
        kp_a.clone(),
        transport_a,
        true,
    );

    // Wait for Alice's listen address
    wait_for_listen_addr(&mut alice,).await;
    let alice_addr = alice.listen_addr.clone().expect("Alice listen addr",);

    let transport_b =
        swarm_task::new(&table_id, kp_b.clone(), Some(alice_addr.clone(),),);
    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        3,
        kp_b.clone(),
        transport_b,
        false,
    );
    wait_for_listen_addr(&mut bob,).await;

    let transport_c =
        swarm_task::new(&table_id, kp_c.clone(), Some(alice_addr.clone(),),);
    let mut charlie = Projection::new(
        "Charlie".into(),
        table_id,
        3,
        kp_c.clone(),
        transport_c,
        false,
    );
    wait_for_listen_addr(&mut charlie,).await;

    // expect the Handphase of each peer to be WaitingForPlayers.
    assert_eq!(alice.phase(), HandPhase::WaitingForPlayers);
    assert_eq!(bob.phase(), HandPhase::WaitingForPlayers);
    assert_eq!(charlie.phase(), HandPhase::WaitingForPlayers);

    // three peers all join.
    // Alice joins
    alice
        .handle_ui_msg(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await;

    pump_three(&mut alice, &mut bob, &mut charlie,).await;
    alice.tick().await;
    bob.tick().await;
    charlie.tick().await;

    assert_eq!(alice.players().len(), 1);
    assert_eq!(charlie.players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
    assert_eq!(bob.players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
    assert_eq!(bob.hash_head(), charlie.hash_head());

    // Bob joins
    bob.handle_ui_msg(UIEvent::PlayerJoinTableRequest {
        table_id,
        player_requesting_join: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(CHIPS_JOIN_AMOUNT,),
    },)
        .await;

    pump_three(&mut alice, &mut bob, &mut charlie,).await;
    alice.tick().await;
    bob.tick().await;
    charlie.tick().await;

    assert_eq!(alice.players().len(), 2);
    assert_eq!(bob.players().len(), 2);
    assert_eq!(charlie.players().len(), 0); // charlie has 0 players, since he has not synced yet.
    assert_eq!(alice.hash_chain().len(), 2);
    assert_eq!(bob.hash_chain().len(), 2);
    assert_eq!(charlie.hash_chain().len(), 0); // charlie has not advanced his hash chain, since he has not synced yet.
    assert_eq!(alice.hash_head(), bob.hash_head());

    // Charlie joins
    charlie
        .handle_ui_msg(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: charlie.peer_id(),
            nickname: "Charlie".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await;

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    assert_eq!(alice.players().len(), 3);
    assert_eq!(bob.players().len(), 3);
    assert_eq!(charlie.players().len(), 3);
    assert_eq!(alice.hash_chain().len(), 3);
    assert_eq!(bob.hash_chain().len(), 3);
    assert_eq!(charlie.players().len(), 3);
    assert_eq!(alice.hash_head(), bob.hash_head());
    assert_eq!(alice.hash_head(), charlie.hash_head());
    assert_eq!(bob.hash_head(), charlie.hash_head());
    assert_eq!(alice.hash_chain(), bob.hash_chain());
    assert_eq!(alice.hash_head(), charlie.hash_head());
    assert_eq!(bob.hash_head(), charlie.hash_head());

    // now after all three peers have joined, we expect the state of the
    // hand phase of each peer to have moved to GameStarting.
    assert_eq!(alice.phase(), HandPhase::StartingGame);
    assert_eq!(bob.phase(), HandPhase::StartingGame);
    assert_eq!(charlie.phase(), HandPhase::StartingGame);

    Ok((),)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn enter_start_hand_test() -> Result<(),> {
    init_logger();
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();
    let kp_c = KeyPair::generate();

    let table_id = TableId::new_id();

    // Alice as seed
    let transport_a = swarm_task::new(&table_id, kp_a.clone(), None,);
    let mut alice = Projection::new(
        "Alice".into(),
        table_id,
        3,
        kp_a.clone(),
        transport_a,
        true,
    );

    // Wait for Alice's listen address
    wait_for_listen_addr(&mut alice,).await;
    let alice_addr = alice.listen_addr.clone().expect("Alice listen addr",);

    let transport_b =
        swarm_task::new(&table_id, kp_b.clone(), Some(alice_addr.clone(),),);
    let mut bob = Projection::new(
        "Bob".into(),
        table_id,
        3,
        kp_b.clone(),
        transport_b,
        false,
    );
    wait_for_listen_addr(&mut bob,).await;

    let transport_c =
        swarm_task::new(&table_id, kp_c.clone(), Some(alice_addr.clone(),),);
    let mut charlie = Projection::new(
        "Charlie".into(),
        table_id,
        3,
        kp_c.clone(),
        transport_c,
        false,
    );
    wait_for_listen_addr(&mut charlie,).await;

    // expect the Handphase of each peer to be WaitingForPlayers.
    assert_eq!(alice.phase(), HandPhase::WaitingForPlayers);
    assert_eq!(bob.phase(), HandPhase::WaitingForPlayers);
    assert_eq!(charlie.phase(), HandPhase::WaitingForPlayers);

    // three peers all join.
    // Alice joins
    alice
        .handle_ui_msg(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await;

    pump_three(&mut alice, &mut bob, &mut charlie,).await;
    alice.tick().await;
    bob.tick().await;
    charlie.tick().await;

    assert_eq!(alice.players().len(), 1);
    assert_eq!(charlie.players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
    assert_eq!(bob.players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
    assert_eq!(bob.hash_head(), charlie.hash_head());

    // Bob joins
    bob.handle_ui_msg(UIEvent::PlayerJoinTableRequest {
        table_id,
        player_requesting_join: bob.peer_id(),
        nickname: "Bob".into(),
        chips: Chips::new(CHIPS_JOIN_AMOUNT,),
    },)
        .await;

    pump_three(&mut alice, &mut bob, &mut charlie,).await;
    alice.tick().await;
    bob.tick().await;
    charlie.tick().await;

    assert_eq!(alice.players().len(), 2);
    assert_eq!(bob.players().len(), 2);
    assert_eq!(charlie.players().len(), 0); // charlie has 0 players, since he has not synced yet.
    assert_eq!(alice.hash_chain().len(), 2);
    assert_eq!(bob.hash_chain().len(), 2);
    assert_eq!(charlie.hash_chain().len(), 0); // charlie has not advanced his hash chain, since he has not synced yet.
    assert_eq!(alice.hash_head(), bob.hash_head());

    // Charlie joins
    charlie
        .handle_ui_msg(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: charlie.peer_id(),
            nickname: "Charlie".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await;

    pump_three(&mut alice, &mut bob, &mut charlie,).await;

    // now after all three peers have joined, we expect the state of the
    // hand phase of each peer to have moved to GameStarting.
    assert_eq!(alice.phase(), HandPhase::StartingGame);
    assert_eq!(bob.phase(), HandPhase::StartingGame);
    assert_eq!(charlie.phase(), HandPhase::StartingGame);
    assert_eq!(alice.players().len(), 3);
    assert_eq!(bob.players().len(), 3);
    assert_eq!(charlie.players().len(), 3);
    assert_eq!(alice.hash_chain().len(), 3);
    assert_eq!(bob.hash_chain().len(), 3);
    assert_eq!(charlie.players().len(), 3);
    assert_eq!(alice.hash_head(), bob.hash_head());
    assert_eq!(alice.hash_head(), charlie.hash_head());
    assert_eq!(bob.hash_head(), charlie.hash_head());
    assert_eq!(alice.hash_chain(), bob.hash_chain());
    assert_eq!(alice.hash_head(), charlie.hash_head());
    assert_eq!(bob.hash_head(), charlie.hash_head());

    // call update() on each player, such that each instance enters
    // start_game().
    alice.update().await;
    bob.update().await;
    charlie.update().await;

    // pump messages such that each player sends & receives the startGameNotify
    // message of the other peers.
    pump_three(&mut alice, &mut bob, &mut charlie,).await;
    alice.tick().await;
    bob.tick().await;
    charlie.tick().await;

    // wait for leader to send batch.
    pump_three(&mut alice, &mut bob, &mut charlie,).await;
    alice.tick().await;
    bob.tick().await;
    charlie.tick().await;

    assert_eq!(alice.phase(), HandPhase::StartingHand);
    assert_eq!(bob.phase(), HandPhase::StartingHand);
    assert_eq!(charlie.phase(), HandPhase::StartingHand);

    Ok((),)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_mock_gui() -> Result<(),> {
    init_logger();

    let table_id = TableId::new_id();
    let mut alice = MockUi::default(None, table_id, "Alice".into(),);
    alice.wait_for_listen_addr().await;
    let alice_addr = alice.get_listen_addr();
    info!("alice addr: {alice_addr:?}");
    let mut bob = MockUi::default(alice_addr.clone(), table_id, "Bob".into(),);
    bob.wait_for_listen_addr().await;
    let mut charlie =
        MockUi::default(alice_addr.clone(), table_id, "Charlie".into(),);
    charlie.wait_for_listen_addr().await;
    let _ = charlie.get_listen_addr();

    // expect the Handphase of each peer to be WaitingForPlayers.
    assert_eq!(
        alice.last_game_state().hand_phase,
        HandPhase::WaitingForPlayers
    );
    assert_eq!(bob.last_game_state().hand_phase, HandPhase::WaitingForPlayers);
    assert_eq!(
        charlie.last_game_state().hand_phase,
        HandPhase::WaitingForPlayers
    );

    // three peers all join.
    // Alice joins
    let res = alice
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice.last_game_state().player_id(),
            nickname: "Alice".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await;
    info!("{res:?}");

    alice.poll_game_state().await;
    bob.poll_game_state().await;
    charlie.poll_game_state().await;

    assert_eq!(alice.last_game_state().players().len(), 1);
    assert_eq!(charlie.last_game_state().players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
    assert_eq!(bob.last_game_state().players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
    assert_eq!(
        bob.last_game_state().hash_head,
        charlie.last_game_state().hash_head
    );

    // Bob joins
    let _ = bob
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: bob.last_game_state().player_id,
            nickname: "Bob".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await;

    alice.poll_game_state().await;
    bob.poll_game_state().await;
    charlie.poll_game_state().await;

    assert_eq!(alice.last_game_state().players().len(), 2);
    assert_eq!(bob.last_game_state().players().len(), 2);
    assert_eq!(charlie.last_game_state().players().len(), 0); // charlie has 0 players, since he has not synced yet.
    assert_eq!(alice.last_game_state().hash_chain.len(), 2);
    assert_eq!(bob.last_game_state().hash_chain.len(), 2);
    assert_eq!(charlie.last_game_state().hash_chain.len(), 0); // charlie has not advanced his hash chain, since he has not synced yet.
    assert_eq!(
        alice.last_game_state().hash_head,
        bob.last_game_state().hash_head
    );

    // Charlie joins
    let _ = charlie
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: charlie.last_game_state().player_id(),
            nickname: "Charlie".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await;

    // now after all three peers have joined, we expect the state of the
    // hand phase of each peer to have moved to GameStarting.
    alice.poll_game_state().await;
    bob.poll_game_state().await;
    charlie.poll_game_state().await;

    let alice_gs = alice.last_game_state();
    let bob_gs = bob.last_game_state();
    let charlie_gs = charlie.last_game_state();

    assert_eq!(alice_gs.hand_phase, HandPhase::StartingGame);
    assert_eq!(bob_gs.hand_phase, HandPhase::StartingGame);
    assert_eq!(charlie_gs.hand_phase, HandPhase::StartingGame);

    assert_eq!(alice_gs.players().len(), 3);
    assert_eq!(bob_gs.players().len(), 3);
    assert_eq!(charlie_gs.players().len(), 3);

    let bob_gs = bob.poll_game_state().await;
    let charlie_gs = charlie.poll_game_state().await;
    let alice_gs = alice.poll_game_state().await;

    //    assert_eq!(alice_gs.hash_chain.len(), 3, "{}", format!("{:?}",
    // alice_gs.hash_chain).to_string()); assert_eq!(charlie_gs.hash_chain.
    // len(), 6, "{}", format!("{:?}", charlie_gs.hash_chain).to_string());
    // assert_eq!(bob_gs.hash_chain.len(), 6, "{}", format!("{:?}",
    // bob_gs.hash_chain).to_string());
    assert_eq!(
        alice_gs.hash_head,
        bob_gs.hash_head,
        "{}",
        format!(
            "\n{}\n{}",
            alice_gs.hash_chain.last().unwrap(),
            bob_gs.hash_chain.last().unwrap()
        )
    );
    assert_eq!(
        alice_gs.hash_head,
        charlie_gs.hash_head,
        "{}",
        format!(
            "\n{}\n{}",
            alice_gs.hash_chain.last().unwrap(),
            charlie_gs.hash_chain.last().unwrap()
        )
    );
    assert_eq!(bob_gs.hash_head, charlie_gs.hash_head);
    assert_eq!(alice_gs.hash_chain, bob_gs.hash_chain);
    assert_eq!(alice_gs.hash_head, charlie_gs.hash_head);
    assert_eq!(bob_gs.hash_head, charlie_gs.hash_head);

    let alice_gs = alice.poll_game_state().await;
    let bob_gs = bob.poll_game_state().await;
    let charlie_gs = charlie.poll_game_state().await;

    assert_eq!(alice_gs.hand_phase, HandPhase::StartingHand);
    assert_eq!(bob_gs.hand_phase, HandPhase::StartingHand);
    assert_eq!(charlie_gs.hand_phase, HandPhase::StartingHand);

    Ok((),)
}
