//! `crates/integration-tests/tests/mock_gui_tests.rs`

mod support;

use crate::support::mock_gui::MockUi;
use anyhow::Result;
use env_logger::Env;
use poker_core::crypto::KeyPair;
use poker_core::game_state::HandPhase;
use poker_core::message::UIEvent;
use poker_core::poker::{Chips, TableId};

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
async fn two_peers_join_success_mock_gui() -> Result<(),> {
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();
    let table_id = TableId::new_id();
    let num_seats = 2;
    let mut alice_ui =
        MockUi::new(kp_a, "Alice".into(), None, num_seats, table_id,);
    alice_ui.wait_for_listen_addr().await;
    let mut bob_ui = MockUi::new(
        kp_b,
        "Bob".into(),
        alice_ui.get_listen_addr(),
        num_seats,
        table_id,
    );
    bob_ui.wait_for_listen_addr().await;

    // Alice joins
    alice_ui
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice_ui.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(1_000,),
        },)
        .await?;

    {
        let alice = alice_ui.poll_game_state().await;
        let bob = bob_ui.poll_game_state().await;
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
    }

    // Bob joins via SyncReq
    bob_ui
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: bob_ui.peer_id(),
            nickname: "Bob".into(),
            chips: Chips::new(1_000,),
        },)
        .await?;

    {
        let alice = alice_ui.poll_game_state().await;
        let bob = bob_ui.poll_game_state().await;
        // Final assertions
        assert_eq!(alice.players().len(), 2, "Alice sees both players");
        assert_eq!(bob.players().len(), 2, "Bob sees both players");
        assert_eq!(alice.hash_head(), bob.hash_head(), "Hashes match");
        assert!(bob.has_joined_table());
        assert_eq!(alice.hash_chain().len(), 2, "Chain: Alice join + Bob join");
        assert_eq!(bob.hash_chain().len(), 2, "Bob replayed chain");
        assert_eq!(alice.phase(), bob.phase(), "Phases match");
    }

    alice_ui.shutdown().await?;
    bob_ui.shutdown().await?;
    Ok((),)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn three_peers_join_success_mock_gui() -> Result<(),> {
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();
    let kp_c = KeyPair::generate();
    let table_id = TableId::new_id();
    let num_seats = 3;

    let mut alice_ui =
        MockUi::new(kp_a, "Alice".into(), None, num_seats, table_id,);
    alice_ui.wait_for_listen_addr().await;

    let mut bob_ui = MockUi::new(
        kp_b,
        "Bob".into(),
        alice_ui.get_listen_addr(),
        num_seats,
        table_id,
    );
    bob_ui.wait_for_listen_addr().await;

    let mut charlie_ui = MockUi::new(
        kp_c,
        "Charlie".into(),
        alice_ui.get_listen_addr(),
        num_seats,
        table_id,
    );
    charlie_ui.wait_for_listen_addr().await;

    // Alice joins
    alice_ui
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice_ui.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await?;

    {
        let alice = alice_ui.poll_game_state().await;
        let bob = bob_ui.poll_game_state().await;
        let charlie = charlie_ui.poll_game_state().await;
        assert_eq!(alice.players().len(), 1);
        assert_eq!(charlie.players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
        assert_eq!(bob.players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
        assert_eq!(bob.hash_head(), charlie.hash_head());
    }

    // Bob joins
    bob_ui
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: bob_ui.peer_id(),
            nickname: "Bob".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await?;

    {
        let alice = alice_ui.poll_game_state().await;
        let bob = bob_ui.poll_game_state().await;
        let charlie = charlie_ui.poll_game_state().await;
        assert_eq!(alice.players().len(), 2);
        assert_eq!(bob.players().len(), 2);
        assert_eq!(charlie.players().len(), 0); // charlie has 0 players, since he has not synced yet.
        assert_eq!(alice.hash_chain().len(), 2);
        assert_eq!(bob.hash_chain().len(), 2);
        assert_eq!(charlie.hash_chain().len(), 0); // charlie has not advanced his hash chain, since he has not synced yet.
        assert_eq!(alice.hash_head(), bob.hash_head());
    }

    // Charlie joins
    charlie_ui
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: charlie_ui.peer_id(),
            nickname: "Charlie".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await?;

    {
        let alice = alice_ui.poll_game_state().await;
        let bob = bob_ui.poll_game_state().await;
        let charlie = charlie_ui.poll_game_state().await;

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
    }

    alice_ui.shutdown().await?;
    bob_ui.shutdown().await?;
    charlie_ui.shutdown().await?;
    Ok((),)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn reject_join_table_full_mock_gui() -> Result<(),> {
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();
    let kp_c = KeyPair::generate();
    let table_id = TableId::new_id();
    let num_seats = 2;

    let mut alice_ui =
        MockUi::new(kp_a, "Alice".into(), None, num_seats, table_id,);
    alice_ui.wait_for_listen_addr().await;

    let mut bob_ui = MockUi::new(
        kp_b,
        "Bob".into(),
        alice_ui.get_listen_addr(),
        2,
        table_id,
    );
    bob_ui.wait_for_listen_addr().await;

    let mut charlie_ui = MockUi::new(
        kp_c,
        "Charlie".into(),
        alice_ui.get_listen_addr(),
        2,
        table_id,
    );
    charlie_ui.wait_for_listen_addr().await;

    // Alice joins
    alice_ui
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice_ui.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await?;
    {
        let alice = alice_ui.poll_game_state().await;
        let bob = bob_ui.poll_game_state().await;
        let charlie = charlie_ui.poll_game_state().await;
        assert_eq!(alice.players().len(), 1);
        assert_eq!(bob.players().len(), 0); // bob has not added alice to his player's list, because he has not synced yet.
        assert_eq!(charlie.players().len(), 0); // charlie has not added alice to his player's list, because he has not synced yet.
    }

    // Bob joins
    bob_ui
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: bob_ui.peer_id(),
            nickname: "Bob".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await?;

    {
        let alice = alice_ui.poll_game_state().await;
        let bob = bob_ui.poll_game_state().await;
        let charlie = charlie_ui.poll_game_state().await;
        assert_eq!(alice.players().len(), 2);
        assert_eq!(bob.players().len(), 2);
        assert_eq!(charlie.players().len(), 0); // charlie has 0 players in his list, because he has not synced, yet.
        assert_eq!(alice.hash_chain().len(), 2);
        assert_eq!(bob.hash_chain().len(), 2);
        assert_eq!(charlie.hash_chain().len(), 0); // charlies hash chain should have length 0, because he has not synced, yet.
        assert_eq!(alice.hash_chain(), bob.hash_chain());
    }

    charlie_ui
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: charlie_ui.peer_id(),
            nickname: "Charlie".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await?;

    {
        let alice = alice_ui.poll_game_state().await;
        let bob = bob_ui.poll_game_state().await;
        let charlie = charlie_ui.poll_game_state().await;

        assert_eq!(
            alice.players().len(),
            2,
            "table is full, charlie has not joined"
        );
        assert_eq!(
            bob.players().len(),
            2,
            "table is full, charlie has not joined"
        );
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
    }

    alice_ui.shutdown().await?;
    bob_ui.shutdown().await?;
    charlie_ui.shutdown().await?;
    Ok((),)
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reject_join_game_started_mock_gui() -> Result<(),> {
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();
    let table_id = TableId::new_id();
    let num_seats = 1;

    let mut alice_ui =
        MockUi::new(kp_a, "Alice".into(), None, num_seats, table_id,);
    alice_ui.wait_for_listen_addr().await;
    let mut bob_ui = MockUi::new(
        kp_b,
        "Bob".into(),
        alice_ui.get_listen_addr(),
        num_seats,
        table_id,
    );
    bob_ui.wait_for_listen_addr().await;

    // Alice joins
    alice_ui
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice_ui.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await?;

    // Bob cannot join, since table is full/game has started
    bob_ui
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: bob_ui.peer_id(),
            nickname: "Bob".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await?;

    {
        let alice = alice_ui.poll_game_state().await;
        let bob = bob_ui.poll_game_state().await;
        assert_eq!(alice.players().len(), 1);
        assert_eq!(bob.players().len(), 0);
        assert!(!bob.has_joined_table());
    }

    alice_ui.shutdown().await?;
    bob_ui.shutdown().await?;
    Ok((),)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reject_join_already_joined_mock_gui() -> Result<(),> {
    let kp_a = KeyPair::generate();
    let kp_b = KeyPair::generate();
    let table_id = TableId::new_id();
    let num_seats = 6;

    let mut alice_ui =
        MockUi::new(kp_a, "Alice".into(), None, num_seats, table_id,);
    alice_ui.wait_for_listen_addr().await;

    let mut bob_ui = MockUi::new(
        kp_b,
        "Bon".into(),
        alice_ui.get_listen_addr(),
        num_seats,
        table_id,
    );
    bob_ui.wait_for_listen_addr().await;

    // Alice joins
    alice_ui
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: alice_ui.peer_id(),
            nickname: "Alice".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await?;

    {
        let alice = alice_ui.poll_game_state().await;
        let bob = bob_ui.poll_game_state().await;
        assert_eq!(alice.players().len(), 1); // expect that alice joined her own instance.
        assert_eq!(bob.players().len(), 0); // expect that alice has not joined bobs instance, because he has not synced yet.
        assert!(!bob.has_joined_table()); // expect that bob has not joined a table, yet.
        assert_ne!(alice.hash_chain(), bob.hash_chain()); // expect that the chains diverge, because bob has not sent a SyncRequest, yet.
    }

    // Bob joins first time
    bob_ui
        .send_to_engine(UIEvent::PlayerJoinTableRequest {
            table_id,
            player_requesting_join: bob_ui.peer_id(),
            nickname: "Bob".into(),
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await?;

    {
        let alice = alice_ui.poll_game_state().await;
        let chain_len_before = alice.hash_chain().len();

        bob_ui
            .send_to_engine(UIEvent::PlayerJoinTableRequest {
                table_id,
                player_requesting_join: bob_ui.peer_id(),
                nickname: "Bob".into(),
                chips: Chips::new(CHIPS_JOIN_AMOUNT,),
            },)
            .await?;

        let alice = alice_ui.poll_game_state().await;
        let bob = bob_ui.poll_game_state().await;
        assert_eq!(alice.players().len(), 2);
        assert_eq!(bob.players().len(), 2);
        assert_eq!(
            alice.hash_chain().len(),
            chain_len_before,
            "No new chain entry"
        );
        assert_eq!(bob.hash_chain(), alice.hash_chain());
    }

    alice_ui.shutdown().await?;
    bob_ui.shutdown().await?;
    Ok((),)
}
