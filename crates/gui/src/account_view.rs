// code based on https://github.com/vincev/freezeout
//! Connection dialog view.
use eframe::egui::{
    Align2, Button, Color32, Context, FontFamily, FontId, Grid, RichText,
    Window, vec2,
};
use log::info;
use poker_core::crypto::PeerId;
use poker_core::game_state::ClientGameState;
use poker_core::message::Message;
use poker_core::poker::Chips;

use crate::{App, ConnectView, GameView, View};

const TEXT_FONT: FontId = FontId::new(16.0, FontFamily::Monospace,);

/// Connect view.
pub struct AccountView {
    player_id:         PeerId,
    nickname:          String,
    game_state:        ClientGameState,
    chips:             Chips,
    error:             String,
    connection_closed: bool,
    table_joined:      bool,
    message:           String,
}

impl AccountView {
    /// Creates a new connect view.
    #[must_use]
    pub fn new(chips: Chips, app: &App,) -> Self {
        Self {
            player_id: *app.player_id(),
            nickname: app.nickname().to_string(),
            game_state: ClientGameState::new(
                *app.player_id(),
                app.nickname().to_string(),
            ),
            chips,
            error: String::default(),
            connection_closed: false,
            table_joined: false,
            message: String::default(),
        }
    }
}

impl View for AccountView {
    fn update(
        &mut self,
        ctx: &Context,
        _frame: &mut eframe::Frame,
        app: &mut App,
    ) {
        while let Some(msg,) = app.try_recv() {
            info!("client received event: {msg:?}");
            match msg.message() {
                Message::PlayerJoined { .. } => {
                    self.table_joined = true;
                },
                Message::NotEnoughChips => {
                    self.message =
                        "Not enough chips to play, reconnect later".to_string();
                },
                Message::NoTablesLeftNotification => {
                    self.message =
                        "All tables are busy, reconnect later".to_string();
                },
                Message::PlayerAlreadyJoined => {
                    self.message = "This player has already joined".to_string();
                },
                _ => {},
            }

            self.game_state.handle_message(msg,);
        }

        Window::new("Account",)
            .collapsible(false,)
            .resizable(false,)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 150.0,),)
            .max_width(400.0,)
            .show(ctx, |ui| {
                ui.group(|ui| {
                    Grid::new("my_grid",)
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

                ui.vertical_centered(|ui| {
                    if !self.message.is_empty() {
                        ui.label(
                            RichText::new(&self.message,)
                                .font(TEXT_FONT,)
                                .color(Color32::RED,),
                        );

                        ui.add_space(10.0,);
                    }

                    let btn = Button::new(
                        RichText::new("Join Table",).font(TEXT_FONT,),
                    );
                    if ui.add_sized(vec2(180.0, 30.0,), btn,).clicked() {
                        let _ = app.sign_and_send(Message::PlayerJoinTableRequest {
                            player_id: self.player_id,
                            nickname:  self.nickname.clone(),
                        },);
                        self.table_joined = true;
                    }
                },);
            },);
    }

    fn next(
        &mut self,
        ctx: &Context,
        frame: &mut eframe::Frame,
        app: &mut App,
    ) -> Option<Box<dyn View,>,> {
        if self.connection_closed {
            Some(Box::new(ConnectView::new(frame.storage(), app,),),)
        } else if self.table_joined {
            let empty_state = ClientGameState::new(
                *app.player_id(),
                app.nickname().to_string(),
            );
            Some(Box::new(GameView::new(
                ctx,
                std::mem::replace(&mut self.game_state, empty_state,),
            ),),)
        } else {
            None
        }
    }
}
