use eframe::egui::{
    Align2, Button, Color32, Context, FontFamily, FontId, RichText, TextEdit,
    Window, vec2,
};
use poker_core::crypto::{KeyPair, PeerId};
use poker_core::message::Message;
use poker_core::poker::Chips;

use crate::{AccountView, App, View};

const TEXT_FONT: FontId = FontId::new(16.0, FontFamily::Monospace,);
const LABEL_FONT: FontId = FontId::new(16.0, FontFamily::Monospace,);

/// Connect view.
#[derive(Default,)]
pub struct ConnectView {
    nickname:          String,
    chips:             Chips,
    error:             String,
    server_joined:     bool,
    key_pair:          KeyPair,
    /// peer id of a player to connect to network.
    discovery_peer_id: String,
}

impl ConnectView {
    /// Creates a new connect view.
    #[must_use]
    pub fn new(
        storage: Option<&dyn eframe::Storage,>,
        app: &App,
        key_pair: KeyPair,
    ) -> Self {
        app.get_storage(storage,)
            .map(|d| {
                Self {
                    nickname: d.nickname,
                    discovery_peer_id: String::default(),
                    chips: Chips::default(),
                    error: String::new(),
                    server_joined: false,
                    key_pair,
                }
            },)
            .unwrap_or_default()
    }

    fn passphrase(&self,) -> String {
        self.key_pair.secret().phrase()
    }
    fn player_id(&self,) -> PeerId {
        self.key_pair.public().to_peer_id()
    }
}

impl View for ConnectView {
    fn update(
        &mut self,
        ctx: &Context,
        _frame: &mut eframe::Frame,
        app: &mut App,
    ) {
        while let Some(msg,) = app.try_recv() {
            let msg: &Message = msg.message();
            if let Message::PlayerJoinedConfirmation {
                chips,
                player_id,
                nickname,
                table_id: _table_id,
            } = msg
            {
                if self.player_id() == *player_id && self.nickname == *nickname
                {
                    self.chips = *chips;
                    self.server_joined = true;
                    self.nickname = nickname.to_string();
                }
            }
        }

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
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Peer-Id",).font(LABEL_FONT,),);
                        TextEdit::singleline(&mut self.discovery_peer_id,)
                            .hint_text("Peer-Id",)
                            .char_limit(100,)
                            .desired_width(30.0,)
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
                            return;
                        }

                        self.server_joined = true;
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
        if self.server_joined {
            Some(Box::new(AccountView::new(self.chips, app,),),)
        } else {
            None
        }
    }
}
