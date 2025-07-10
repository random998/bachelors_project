// crates/gui/src/account_view.rs
//
// “Account” screen – shows local identity & lets the player join a table.
// This version is totally lock-free: the GUI owns its own copy of the
// InternalTableState (stored in `App`), and only *reads* from it.

use eframe::egui::{
    Align2, Button, Color32, Context, FontFamily, FontId, Grid, RichText,
    Window, vec2,
};
use log::info;
use poker_core::crypto::PeerId;
use poker_core::game_state::GameState;
use poker_core::message::Message;
use poker_core::poker::{Chips, TableId};

use crate::{App, ConnectView, GameView, View};

const TEXT_FONT: FontId = FontId::new(16.0, FontFamily::Monospace,);

/// First screen after connecting – shows account & lets the player join.
pub struct AccountView {
    player_id:  PeerId,
    nickname:   String,
    game_state: GameState,
    chips:      Chips,

    // ui-only state
    error:             String,
    info_msg:          String,
    table_joined:      bool,
    connection_closed: bool,
}

impl AccountView {
    #[must_use]
    pub fn new(chips: Chips, app: &App,) -> Self {
        Self {
            player_id: *app.player_id(),
            nickname: app.nickname().to_owned(),
            game_state: app.snapshot(), // initial snapshot
            chips,
            error: String::new(),
            info_msg: String::new(),
            table_joined: false,
            connection_closed: false,
        }
    }
}

impl View for AccountView {
    fn update(
        &mut self, ctx: &Context, _f: &mut eframe::Frame, app: &mut App,
    ) {
        // ── 1. pick up the latest state snapshot (if any) ─────────────
        self.game_state = app.snapshot();

        // ── 2. pull echo messages (for status info) ───────────────────
        while let Some(sm,) = app.try_recv() {
            match sm.message() {
                Message::PlayerJoinedConfirmation { .. } => {
                    self.table_joined = true;
                },
                Message::NotEnoughChips { .. } => {
                    self.info_msg =
                        "Not enough chips – come back later.".into();
                },
                Message::NoTablesLeftNotification { .. } => {
                    self.info_msg =
                        "All tables are busy – try again later.".into();
                },
                Message::PlayerAlreadyJoined { .. } => {
                    self.info_msg =
                        "You are already seated at that table.".into();
                },
                _ => {},
            }
        }

        // ── 2. draw the account window ────────────────────────────
        Window::new("Account",)
            .collapsible(false,)
            .resizable(false,)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 150.0,),)
            .max_width(400.0,)
            .show(ctx, |ui| {
                // summary ----------------------------------------------------
                ui.group(|ui| {
                    Grid::new("acc_grid",)
                        .num_columns(2,)
                        .spacing([40.0, 4.0,],)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Nickname",).font(TEXT_FONT,),
                            );
                            ui.label(
                                RichText::new(&self.nickname,).font(TEXT_FONT,),
                            );
                            ui.end_row();

                            ui.label(
                                RichText::new("Player ID",).font(TEXT_FONT,),
                            );
                            ui.label(
                                RichText::new(self.player_id.to_string(),)
                                    .font(TEXT_FONT,),
                            );
                            ui.end_row();

                            ui.label(RichText::new("Chips",).font(TEXT_FONT,),);
                            ui.label(
                                RichText::new(self.chips.to_string(),)
                                    .font(TEXT_FONT,),
                            );
                            ui.end_row();
                        },);
                },);

                ui.add_space(10.0,);

                // info / error line -----------------------------------------
                if !self.info_msg.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new(&self.info_msg,)
                                .font(TEXT_FONT,)
                                .color(Color32::RED,),
                        );
                    },);
                    ui.add_space(10.0,);
                }

                // join table button -----------------------------------------
                ui.vertical_centered(|ui| {
                    let join_btn = Button::new(
                        RichText::new("Join Table",).font(TEXT_FONT,),
                    );

                    if ui.add_sized(vec2(180.0, 30.0,), join_btn,).clicked()
                        && !self.table_joined
                    {
                        let table_id: TableId = app.snapshot().table_id;
                        let join = Message::PlayerJoinTableRequest {
                            table_id,
                            player_id: self.player_id,
                            nickname: self.nickname.clone(),
                            chips: self.chips,
                        };
                        if let Err(e,) = app.sign_and_send(join,) {
                            self.error = format!("send failed: {e}");
                            info!("{}", self.error);
                        }
                    }
                },);

                if !self.error.is_empty() {
                    ui.label(
                        RichText::new(&self.error,)
                            .font(TEXT_FONT,)
                            .color(Color32::RED,),
                    );
                }
            },);
    }

    fn next(
        &mut self,
        ctx: &Context,
        frame: &mut eframe::Frame,
        app: &mut App,
    ) -> Option<Box<dyn View,>,> {
        if self.connection_closed {
            Some(Box::new(ConnectView::new(
                frame.storage(),
                app,
                app.key_pair(),
            ),),)
        } else if self.table_joined {
            let empty_state = GameState::default();
            Some(Box::new(GameView::new(
                ctx,
                std::mem::replace(&mut self.game_state, empty_state,),
            ),),)
        } else {
            None
        }
    }
}
