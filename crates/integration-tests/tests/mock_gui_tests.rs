//! `crates/integration-tests/tests/mock_gui_tests.rs`
mod support;
use anyhow::Result;
use env_logger::Env;
use poker_core::game_state::HandPhase;
use poker_core::message::UIEvent;
use poker_core::poker::{Chips, TableId};

use crate::support::mock_gui::MockUi;

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
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn two_peers_join_success_mock_gui() -> Result<(),> {
    init_logger();
    let table_id = TableId::new_id();
    let num_seats = 3;

    let mut alice_ui =
        MockUi::new("Alice".into(), None, num_seats, table_id,);
    alice_ui.wait_for_listen_addr().await;

    let mut bob_ui = MockUi::new(
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

    {
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        // Assertions after Alice joins
        assert_eq!(alice.get_players().len(), 1, "Alice should see herself");
        assert_eq!(
            bob.get_players().len(),
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
            chips: Chips::new(CHIPS_JOIN_AMOUNT,),
        },)
        .await?;

    {
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        // Final assertions
        assert_eq!(alice.get_players().len(), 2, "Alice sees both players");
        assert_eq!(bob.get_players().len(), 2, "Bob sees both players");
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
    let table_id = TableId::new_id();
    let num_seats = 3;

    let mut alice_ui =
        MockUi::new("Alice".into(), None, num_seats, table_id,);
    alice_ui.wait_for_listen_addr().await;

    let mut bob_ui = MockUi::new(
        "Bob".into(),
        alice_ui.get_listen_addr(),
        num_seats,
        table_id,
    );
    bob_ui.wait_for_listen_addr().await;

    let mut charlie_ui = MockUi::new(
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
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        let charlie = charlie_ui.last_game_state().await;
        assert_eq!(alice.get_players().len(), 1);
        assert_eq!(charlie.get_players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
        assert_eq!(bob.get_players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
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
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        let charlie = charlie_ui.last_game_state().await;
        assert_eq!(alice.get_players().len(), 2);
        assert_eq!(bob.get_players().len(), 2);
        assert_eq!(charlie.get_players().len(), 0); // charlie has 0 players, since he has not synced yet.
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
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        let charlie = charlie_ui.last_game_state().await;

        assert_eq!(alice.get_players().len(), 3);
        assert_eq!(bob.get_players().len(), 3);
        assert_eq!(charlie.get_players().len(), 3);
        assert_eq!(alice.hash_chain().len(), 3);
        assert_eq!(bob.hash_chain().len(), 3);
        assert_eq!(charlie.get_players().len(), 3);
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)]
async fn reject_join_table_full_mock_gui() -> Result<(),> {
    let table_id = TableId::new_id();
    let num_seats = 2;

    let mut alice_ui =
        MockUi::new("Alice".into(), None, num_seats, table_id,);
    alice_ui.wait_for_listen_addr().await;

    let mut bob_ui = MockUi::new(
        "Bob".into(),
        alice_ui.get_listen_addr(),
        num_seats,
        table_id,
    );
    bob_ui.wait_for_listen_addr().await;

    let mut charlie_ui = MockUi::new(
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
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        let charlie = charlie_ui.last_game_state().await;
        assert_eq!(alice.get_players().len(), 1);
        assert_eq!(bob.get_players().len(), 0); // bob has not added alice to his player's list, because he has not synced yet.
        assert_eq!(charlie.get_players().len(), 0); // charlie has not added alice to his player's list, because he has not synced yet.
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
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        let charlie = charlie_ui.last_game_state().await;
        assert_eq!(alice.get_players().len(), 2);
        assert_eq!(charlie.get_players().len(), 0); // charlie has 0 players in his list, because he has not synced, yet.
        assert_eq!(alice.hash_chain().len(), 2);
        assert_eq!(bob.hash_chain().len(), 2);
        assert_eq!(charlie.hash_chain().len(), 0); // charlies hash chain should have length 0, because he has not synced, yet.
        assert_eq!(alice.hash_chain(), bob.hash_chain());
        assert_eq!(bob.get_players().len(), 2);
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
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        let charlie = charlie_ui.last_game_state().await;

        assert_eq!(
            alice.get_players().len(),
            2,
            "table is full, charlie has not joined"
        );
        assert_eq!(
            bob.get_players().len(),
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
            "bobs hash chain: {:?}",
            bob.hash_chain.clone(),
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
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn reject_join_game_started_mock_gui() -> Result<(),> {
    let table_id = TableId::new_id();
    let num_seats = 1;

    let mut alice_ui =
        MockUi::new("Alice".into(), None, num_seats, table_id,);
    alice_ui.wait_for_listen_addr().await;
    let mut bob_ui = MockUi::new(
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
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        assert_eq!(alice.get_players().len(), 1);
        assert_eq!(bob.get_players().len(), 0);
        assert!(!bob.has_joined_table());
    }

    alice_ui.shutdown().await?;
    bob_ui.shutdown().await?;
    Ok((),)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reject_join_already_joined_mock_gui() -> Result<(),> {
    init_logger();

    let table_id = TableId::new_id();
    let num_seats = 3;

    let mut alice_ui =
        MockUi::new("Alice".into(), None, num_seats, table_id,);
    alice_ui.wait_for_listen_addr().await;

    let mut bob_ui = MockUi::new(
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

    {
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        assert_eq!(alice.get_players().len(), 1); // expect that alice joined her own instance.
        assert_eq!(bob.get_players().len(), 0); // expect that alice has not joined bobs instance, because he has not synced yet.
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
        let alice = alice_ui.last_game_state().await;
        let chain_len_before = alice.hash_chain().len();

        bob_ui
            .send_to_engine(UIEvent::PlayerJoinTableRequest {
                table_id,
                player_requesting_join: bob_ui.peer_id(),
                nickname: "Bob".into(),
                chips: Chips::new(CHIPS_JOIN_AMOUNT,),
            },)
            .await?;

        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        assert_eq!(alice.get_players().len(), 2);
        assert_eq!(bob.get_players().len(), 2);
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

/// tests whether the same cards are dealt at each instance.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_card_dealing() -> Result<(),> {
    // setup: 3 peers join
    let table_id = TableId::new_id();
    let num_seats = 3;

    let mut alice_ui =
        MockUi::new("Alice".into(), None, num_seats, table_id,);
    alice_ui.wait_for_listen_addr().await;

    let mut bob_ui = MockUi::new(
        "Bob".into(),
        alice_ui.get_listen_addr(),
        num_seats,
        table_id,
    );
    bob_ui.wait_for_listen_addr().await;

    let mut charlie_ui = MockUi::new(
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
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        let charlie = charlie_ui.last_game_state().await;
        assert_eq!(alice.get_players().len(), 1);
        assert_eq!(charlie.get_players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
        assert_eq!(bob.get_players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
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
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        let charlie = charlie_ui.last_game_state().await;
        assert_eq!(alice.get_players().len(), 2);
        assert_eq!(bob.get_players().len(), 2);
        assert_eq!(charlie.get_players().len(), 0); // charlie has 0 players, since he has not synced yet.
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
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        let charlie = charlie_ui.last_game_state().await;

        assert_eq!(alice.get_players().len(), 3);
        assert_eq!(bob.get_players().len(), 3);
        assert_eq!(charlie.get_players().len(), 3);
        assert_eq!(alice.hash_chain().len(), 3);
        assert_eq!(bob.hash_chain().len(), 3);
        assert_eq!(charlie.get_players().len(), 3);
        assert_eq!(alice.hash_head(), bob.hash_head());
        assert_eq!(alice.hash_head(), charlie.hash_head());
        assert_eq!(bob.hash_head(), charlie.hash_head());
        assert_eq!(alice.hash_chain(), bob.hash_chain());
        assert_eq!(alice.hash_head(), charlie.hash_head());
        assert_eq!(bob.hash_head(), charlie.hash_head());
    }

    {
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        let charlie = charlie_ui.last_game_state().await;
        assert_eq!(alice.get_players().len(), 3);
        assert_eq!(bob.get_players().len(), 3);
        assert_eq!(charlie.get_players().len(), 3);
        // Sort players by peer_id for consistent order across instances
        let mut alice_players = alice.get_players();
        alice_players.sort_by_key(|p| p.peer_id.to_string(),);
        let mut bob_players = bob.get_players();
        bob_players.sort_by_key(|p| p.peer_id.to_string(),);
        let mut charlie_players = charlie.get_players();
        charlie_players.sort_by_key(|p| p.peer_id.to_string(),);
        let alice_player1 = alice_players.first().unwrap();
        let alice_player2 = alice_players.get(1,).unwrap();
        let alice_player3 = alice_players.get(2,).unwrap();
        let bob_player1 = bob_players.first().unwrap();
        let bob_player2 = bob_players.get(1,).unwrap();
        let bob_player3 = bob_players.get(2,).unwrap();
        let charlie_player1 = charlie_players.first().unwrap();
        let charlie_player2 = charlie_players.get(1,).unwrap();
        let charlie_player3 = charlie_players.get(2,).unwrap();
        // Check cards for each sorted position (now consistent by peer_id)
        // Player 1 (same peer_id across)
        assert_eq!(alice_player1.peer_id, bob_player1.peer_id);
        assert_eq!(alice_player1.peer_id, charlie_player1.peer_id);
        assert_eq!(alice_player1.hole_cards, bob_player1.hole_cards);
        assert_eq!(alice_player1.hole_cards, charlie_player1.hole_cards);
        assert_eq!(alice_player1.public_cards, bob_player1.public_cards);
        assert_eq!(alice_player1.public_cards, charlie_player1.public_cards);
        // Player 2
        assert_eq!(alice_player2.peer_id, bob_player2.peer_id);
        assert_eq!(alice_player2.peer_id, charlie_player2.peer_id);
        assert_eq!(alice_player2.hole_cards, bob_player2.hole_cards);
        assert_eq!(alice_player2.hole_cards, charlie_player2.hole_cards);
        assert_eq!(alice_player2.public_cards, bob_player2.public_cards);
        assert_eq!(alice_player2.public_cards, charlie_player2.public_cards);
        // Player 3
        assert_eq!(alice_player3.peer_id, bob_player3.peer_id);
        assert_eq!(alice_player3.peer_id, charlie_player3.peer_id);
        assert_eq!(alice_player3.hole_cards, bob_player3.hole_cards);
        assert_eq!(alice_player3.hole_cards, charlie_player3.hole_cards);
        assert_eq!(alice_player3.public_cards, bob_player3.public_cards);
        assert_eq!(alice_player3.public_cards, charlie_player3.public_cards);
    }

    // clean up && shutdown
    alice_ui.shutdown().await?;
    bob_ui.shutdown().await?;
    charlie_ui.shutdown().await?;
    Ok((),)
}
/// tests whether the local player is the first in the player list at each
/// instance.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_local_player_first() -> Result<(),> {
    // setup: 3 peers join
    let table_id = TableId::new_id();
    let num_seats = 3;

    let mut alice_ui =
        MockUi::new("Alice".into(), None, num_seats, table_id,);
    alice_ui.wait_for_listen_addr().await;

    let mut bob_ui = MockUi::new(
        "Bob".into(),
        alice_ui.get_listen_addr(),
        num_seats,
        table_id,
    );
    bob_ui.wait_for_listen_addr().await;

    let mut charlie_ui = MockUi::new(
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
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        let charlie = charlie_ui.last_game_state().await;
        assert_eq!(alice.get_players().len(), 1);
        assert_eq!(charlie.get_players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
        assert_eq!(bob.get_players().len(), 0); // we expect that charlie has 0 players, since he rejects any logentries since he has not synced yet.
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
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        let charlie = charlie_ui.last_game_state().await;
        assert_eq!(alice.get_players().len(), 2);
        assert_eq!(bob.get_players().len(), 2);
        assert_eq!(charlie.get_players().len(), 0); // charlie has 0 players, since he has not synced yet.
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
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        let charlie = charlie_ui.last_game_state().await;

        assert_eq!(alice.get_players().len(), 3);
        assert_eq!(bob.get_players().len(), 3);
        assert_eq!(charlie.get_players().len(), 3);
        assert_eq!(alice.hash_chain().len(), 3);
        assert_eq!(bob.hash_chain().len(), 3);
        assert_eq!(charlie.get_players().len(), 3);
        assert_eq!(alice.hash_head(), bob.hash_head());
        assert_eq!(alice.hash_head(), charlie.hash_head());
        assert_eq!(bob.hash_head(), charlie.hash_head());
        assert_eq!(alice.hash_chain(), bob.hash_chain());
        assert_eq!(alice.hash_head(), charlie.hash_head());
        assert_eq!(bob.hash_head(), charlie.hash_head());
    }

    {
        let alice = alice_ui.last_game_state().await;
        let bob = bob_ui.last_game_state().await;
        let charlie = charlie_ui.last_game_state().await;
        assert_eq!(alice.get_players().len(), 3);
        assert_eq!(bob.get_players().len(), 3);
        assert_eq!(charlie.get_players().len(), 3);
        assert_eq!(alice.players.first().unwrap().peer_id, alice.player_id);
        assert_eq!(bob.players.first().unwrap().peer_id, bob.player_id);
        assert_eq!(charlie.players.first().unwrap().peer_id, charlie.player_id);
    }

    // clean up && shutdown
    alice_ui.shutdown().await?;
    bob_ui.shutdown().await?;
    charlie_ui.shutdown().await?;
    Ok((),)
}
