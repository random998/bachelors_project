use eframe::egui::{
    Align2, Button, Color32, Context, Event, FontFamily, FontId, RichText, TextBuffer, TextEdit,
    Window, vec2,
};
use log::error;
use poker_core::crypto::{PeerId, SigningKey};
use poker_core::message::Message;
use poker_core::poker::Chips;

use crate::{AccountView, App, AppData, ConnectionEvent, View};

const TEXT_FONT: FontId = FontId::new(16.0, FontFamily::Monospace,);
const LABEL_FONT: FontId = FontId::new(16.0, FontFamily::Monospace,);

/// Connect view.
#[derive(Default)]
pub struct ConnectView {
    nickname: String,
    chips: Chips,
    error: String,
    server_joined: bool,
    signing_key: SigningKey, // key for signing messages
}


impl ConnectView {
    /// Creates a new connect view.
    #[must_use]
    pub fn new(storage: Option<&dyn eframe::Storage,>, app: &App,) -> Self {
        app.get_storage(storage,)
            .map(|d| {
                let sk = SigningKey::from_phrase(&d.passphrase,).unwrap_or_default();
                Self {
                    nickname: d.nickname,
                    chips: Chips::default(),
                    error: String::new(),
                    server_joined: false,
                    signing_key: sk,
                }
            },)
            .unwrap_or_default()
    }

    fn passphrase(&self,) -> String {
        self.signing_key.phrase()
    }
    fn player_id(&self,) -> PeerId {
        self.signing_key.verifying_key().peer_id()
    }

    fn assign_key(&mut self, sk: &SigningKey,) {
        self.signing_key = sk.clone();
    }
}

impl View for ConnectView {
    fn update(&mut self, ctx: &Context, frame: &mut eframe::Frame, app: &mut App,) {
        while let Some(event,) = app.poll_network() {
            match event {
                | ConnectionEvent::Open => {
                    app.send_message(Message::JoinTableRequest {
                        player_id: self.player_id(),
                        nickname: self.nickname.to_string(),
                    },);
                },
                | ConnectionEvent::Close => {
                    self.error = "Connection closed".to_string();
                },
                | ConnectionEvent::Error(e,) => {
                    self.error = format!("Connection error {e}");
                },
                | ConnectionEvent::Message(msg,) => {
                    if let Message::PlayerJoined {
                        table_id: _table_id,
                        player_id,
                        nickname,
                        chips,
                    } = msg.message()
                    {
                        if self.player_id() == *player_id {
                            self.chips = *chips;
                            self.server_joined = true;
                            self.nickname = nickname.to_string();
                        }
                    }
                },
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
                },);

                ui.add_space(10.0,);

                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Private Passphrase",).font(LABEL_FONT,),);
                        ui.add_space(125.0,);
                        if ui.button(RichText::new("Generate",).font(TEXT_FONT,),).clicked() {
                            self.error.clear();
                            let sk = SigningKey::default();
                            self.assign_key(&sk,);
                        }
                    },);

                    // Copy passphrase from clipboard by replacing current text
                    ui.input(|i| {
                        for event in &i.events {
                            if let Event::Paste(text,) = event {
                                if let Ok(sk,) = SigningKey::from_phrase(text,) {
                                    self.error.clear();
                                    self.assign_key(&sk,);
                                } else {
                                    self.error = "Invalid clipboard passphrase".to_string();
                                }
                            }
                        }
                    },);

                    // Copy field value to avoid editing, these fields can only be
                    // changed by pasting the passphrase or with the generate button.
                    let mut passphrase = self.passphrase();
                    TextEdit::multiline(&mut passphrase,)
                        .char_limit(108,)
                        .desired_rows(3,)
                        .desired_width(400.0,)
                        .font(TEXT_FONT,)
                        .show(ui,);
                },);

                ui.add_space(10.0,);

                ui.vertical_centered(|ui| {
                    if !self.error.is_empty() {
                        ui.label(
                            RichText::new(&self.error)
                                .font(TEXT_FONT)
                                .color(Color32::RED),
                        );

                        ui.add_space(5.0,);
                    }

                    let btn = Button::new(RichText::new("Connect",).font(TEXT_FONT,),);
                    if ui.add_sized(vec2(120.0, 30.0,), btn,).clicked() {
                        self.error.clear();

                        if self.nickname.trim().is_empty() {
                            self.error = "Invalid nickname".to_string();
                            return;
                        }

                        let sk = if let Ok(sk,) = SigningKey::from_phrase(&self.passphrase(),) {
                            let data = AppData {
                                passphrase: self.passphrase(),
                                nickname: self.nickname.clone(),
                            };

                            app.set_storage(frame.storage_mut(), &data,);

                            sk
                        } else {
                            self.error = "Invalid passphrase".to_string();
                            return;
                        };

                        if let Err(e,) = app.connect(sk, self.nickname.trim(), ctx,) {
                            self.error = "Connect error".to_string();
                            error!("Connect error: {e}");
                        }
                    }
                },);
            },);
    }
    fn next(
        &mut self, _ctx: &Context, _frame: &mut eframe::Frame, app: &mut App,
    ) -> Option<Box<dyn View,>,> {
        if self.server_joined {
            Some(Box::new(AccountView::new(self.chips, app,),),)
        } else {
            None
        }
    }
}
