use std::time::Duration;

use libp2p::Multiaddr;
use log::info;
use p2p_net::runtime_bridge;
use p2p_net::runtime_bridge::UiHandle;
use poker_core::crypto::KeyPair;
use poker_core::game_state::GameState;
use poker_core::message::{EngineEvent, UIEvent};
use poker_core::poker::TableId;
use tokio::time::sleep;

pub struct MockUi {
    ui_handle:      UiHandle,
    last_gamestate: GameState,
}

impl MockUi {
    pub fn new(
        keypair: KeyPair,
        nick: String,
        seed_addr: Option<Multiaddr,>,
        num_seats: usize,
        table_id: TableId,
    ) -> Self {
        let ui = runtime_bridge::start(
            table_id,
            num_seats,
            keypair,
            nick,
            seed_addr,
        );

        Self {
            ui_handle:      ui,
            last_gamestate: GameState::default(),
        }
    }

    pub fn default(
        seed_addr: Option<Multiaddr,>,
        table_id: TableId,
        nick: String,
    ) -> Self {
        let kp = KeyPair::generate();

        Self::new(kp, nick, seed_addr, 3, table_id,)
    }

    pub async fn send_to_engine(
        &mut self,
        ui_cmd: UIEvent,
    ) -> Result<(), anyhow::Error,> {
        let err = self
            .ui_handle
            .cmd_tx
            .send(ui_cmd,)
            .await
            .map_err(std::convert::Into::into,);
        // update to pull the latest gamestate after sending msg to engine.
        self.poll_game_state().await;
        err
    }

    pub fn try_recv_from_engine(
        &mut self,
    ) -> Result<EngineEvent, anyhow::Error,> {
        self.ui_handle.msg_rx.try_recv().map_err(std::convert::Into::into,)
    }

    pub async fn wait_for_listen_addr(&mut self,) {
        let mut attempts = 0;
        let max_attempts = 20;
        loop {
            while let Ok(ui_event,) = self.try_recv_from_engine() {
                if let EngineEvent::Snapshot(game_state,) = ui_event {
                    self.last_gamestate = game_state;
                }
            }
            if self.last_gamestate.listen_addr.is_some() {
                break;
            }
            sleep(Duration::from_millis(50,),).await;
            attempts += 1;
            assert!(
                attempts <= max_attempts,
                "Timeout waiting for listen addr"
            );
        }
    }

    pub fn get_listen_addr(&self,) -> Option<Multiaddr,> {
        self.last_gamestate.listen_addr.clone()
    }

    pub fn last_game_state(&self,) -> GameState {
        self.last_gamestate.clone()
    }

    pub async fn poll_game_state(&mut self,) -> GameState {
        let mut tries = 0u64;
        let max_tries = 100;
        let delay_ms = 20;
        loop {
            while let Ok(res,) = self.try_recv_from_engine() {
                if let EngineEvent::Snapshot(gs,) = res {
                    if self.last_gamestate != gs {
                        self.last_gamestate = gs;
                        return self.last_gamestate.clone();
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(delay_ms, ), ).await;
            tries += 1;
            if tries >= max_tries {
                info!(
                    "{} did not update gamestate",
                    self.last_gamestate.nickname.clone()
                );
                return self.last_gamestate.clone();
            }
        }
    }
}
