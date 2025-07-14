use eframe::egui::{
    Align2, Button, Color32, Context, FontFamily, FontId, RichText, TextEdit,
    Window, vec2,
};
use poker_core::crypto::PeerId;
use poker_core::game_state::GameState;
use poker_core::message::UiCmd;
use poker_core::poker::Chips;

use crate::{AccountView, App, View};

const TEXT_FONT: FontId = FontId::new(16.0, FontFamily::Monospace,);
const LABEL_FONT: FontId = FontId::new(16.0, FontFamily::Monospace,);

/// Connect view.
pub struct ConnectView {
    game_state: GameState,
    error:      String,
    nickname:   String,
    chips: Chips
}

impl ConnectView {
    const fn joined_server(&self,) -> bool {
        self.game_state.has_joined_server
    }

    fn update_chips(&mut self,){
        let us = self.game_state.players.get_mut(&self.player_id());
        if let Some(p) = us {
            self.chips = p.chips;
        }
    }

    const fn peer_id(&self,) -> PeerId {
        self.game_state.player_id
    }
}

impl ConnectView {
    /// Creates a new connect view.
    #[must_use]
    pub fn new(app: &App,) -> Self {
        let gs = app.game_state.clone();
        Self {
            game_state: gs,
            error:      String::default(),
            nickname:   String::default(),
            chips:      Chips::default(),
        }
    }

    fn passphrase(&self,) -> String {
        self.game_state.key_pair.secret().phrase()
    }
    fn player_id(&self,) -> PeerId {
        self.game_state.key_pair.public().to_peer_id()
    }
}

impl View for ConnectView {
    fn update(
        &mut self,
        ctx: &Context,
        _frame: &mut eframe::Frame,
        app: &mut App,
    ) {
        self.game_state = app.game_state.clone();
        self.update_chips();

        Window::new("Login",)
            .collapsible(false,)
            .resizable(false,)
            .anchor(Align2::CENTER_TOP, vec2(0.0, 150.0,),)
            .max_width(400.0,)
            .show(ctx, |ui| {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Nickname",).font(LABEL_FONT,),);
                        TextEdit::singleline(&mut self.nickname,)
                            .hint_text("Nickname",)
                            .char_limit(10,)
                            .desired_width(310.0,)
                            .font(TEXT_FONT,)
                            .show(ui,);
                    },);
                },);

                ui.add_space(10.0,);

                ui.vertical_centered(|ui| {
                    if !self.error.is_empty() {
                        ui.label(
                            RichText::new(&self.error,)
                                .font(TEXT_FONT,)
                                .color(Color32::RED,),
                        );

                        ui.add_space(5.0,);
                    }

                    let btn = Button::new(
                        RichText::new("Connect",).font(TEXT_FONT,),
                    );
                    if ui.add_sized(vec2(120.0, 30.0,), btn,).clicked() {
                        self.error.clear();

                        if self.nickname.trim().is_empty() {
                            self.error = "Invalid nickname".to_string();
                        }

                        self.game_state.nickname = self.nickname.clone();
                        let msg = UiCmd::PlayerJoinTableRequest {
                            table_id:  self.game_state.table_id,
                            player_id: self.player_id(),
                            nickname:  self.nickname.clone(),
                            chips:     self.chips,
                        };

                        let _ = app.send_cmd_to_engine(msg,);
                    }
                },);
            },);
    }
    fn next(
        &mut self,
        _ctx: &Context,
        _frame: &mut eframe::Frame,
        app: &mut App,
    ) -> Option<Box<dyn View,>,> {
        if self.joined_server() {
            Some(Box::new(AccountView::new(self.chips, app,),),)
        } else {
            None
        }
    }
}
