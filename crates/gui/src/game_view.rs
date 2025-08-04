// crates/gui/src/account_view.rs
// -----------------------------------------------------------------------------
//  “Game View” — table rendering + input handling
//  (file name kept from the original freezeout example)
// -----------------------------------------------------------------------------

use eframe::egui::text::LayoutJob;
use eframe::egui::{
    Align, Align2, Button, Color32, Context, CornerRadius, FontFamily, FontId,
    Image, Key, Pos2, Rect, RichText, Sense, Slider, Stroke, StrokeKind,
    TextFormat, Ui, Vec2, Window, pos2, text, vec2,
};
use eframe::epaint;
use poker_cards::egui::Textures;
use poker_core::game_state::{GameState, PlayerPrivate};
use poker_core::message::{PlayerAction, UIEvent};
use poker_core::poker::{Chips, PlayerCards};

use crate::{AccountView, App, ConnectView, View};

// ─────────────────────────────── configuration ────────────────────────────

const TEXT_FONT: FontId = FontId::new(15.0, FontFamily::Monospace,);
const SMALL_BUTTON_SZ: Vec2 = vec2(30.0, 30.0,);
const BG_COLOR: Color32 = Color32::from_gray(20,);
const TEXT_COLOR: Color32 = Color32::from_rgb(20, 150, 20,);

// ───────────────────────────── view state  ───────────────────────────────
#[allow(missing_docs, dead_code)]
pub struct GameView {
    connection_closed: bool,
    error:             Option<String,>,
    bet_params:        Option<BetParams,>,
    show_account:      Option<Chips,>,
    show_legend:       bool,
    game_state:        GameState,
}

struct BetParams {
    min_raise:   u32,
    big_blind:   u32,
    raise_value: u32,
}

// ───────────────────────────── constructor  ──────────────────────────────

impl GameView {
    const TEXT_COLOR: Color32 = Color32::from_rgb(20, 150, 20,);
    const TEXT_FONT: FontId = FontId::new(15.0, FontFamily::Monospace,);
    const BG_COLOR: Color32 = Color32::from_gray(20,);
    const ACTION_BUTTON_LX: f32 = 81.0;
    const ACTION_BUTTON_LY: f32 = 35.0;
    #[must_use]
    #[allow(missing_docs)]
    pub fn new(ctx: &Context, game_state: GameState,) -> Self {
        ctx.request_repaint(); // keep the UI animating
        Self {
            game_state,
            connection_closed: false,
            error: None,
            bet_params: None,
            show_account: None,
            show_legend: false,
        }
    }
}

// ───────────────────────────── View trait  ───────────────────────────────

impl View for GameView {
    fn update(
        &mut self, ctx: &Context, _f: &mut eframe::Frame, app: &mut App,
    ) {
        // ── 1. pick up the latest state snapshot (if any) ─────────────
        self.game_state = app.snapshot();

        // ---- 2) draw everything ------------------------------------
        Window::new("zk-poker",)
            .title_bar(false,)
            .collapsible(false,)
            .resizable(false,)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO,)
            .show(ctx, |ui| {
                let (rect, _,) = ui
                    .allocate_exact_size(vec2(1024.0, 640.0,), Sense::hover(),);
                self.paint_table(ui, &rect,);
                self.paint_board(ui, &rect, app,);
                self.paint_pot(ui, &rect,);
                self.paint_players(ui, &rect, app,);
                self.paint_help_button(ui, &rect,);
                self.paint_legend(ui, &rect,);
                self.paint_local_peer_id(ui, &rect,);
                self.paint_hand_phase(ui, &rect,);
                self.paint_listen_addr(ui, &rect,);
            },);
    }

    fn next(
        &mut self,
        _ctx: &Context,
        _frame: &mut eframe::Frame,
        app: &mut App,
    ) -> Option<Box<dyn View,>,> {
        if self.connection_closed {
            Some(Box::new(ConnectView::new(app,),),)
        } else if let Some(chips,) = self.show_account.take() {
            Some(Box::new(AccountView::new(chips, app,),),)
        } else {
            None
        }
    }
}

// ───────────────────────────── painting helpers ──────────────────────────

impl GameView {
    fn paint_player_action(
        &self,
        player: &PlayerPrivate,
        ui: &Ui,
        rect: &Rect,
        align: &Align2,
    ) {
        if matches!(player.hole_cards, PlayerCards::None) {
            return;
        }

        let rect = match align.x() {
            Align::RIGHT => {
                Rect::from_min_size(
                    rect.left_bottom() + vec2(-(rect.width() + 10.0), 10.0,),
                    vec2(rect.width(), 40.0,),
                )
            },
            _ => {
                Rect::from_min_size(
                    rect.left_bottom() + vec2(rect.width() + 10.0, 10.0,),
                    vec2(rect.width(), 40.0,),
                )
            },
        };

        paint_border(ui, &rect,);

        if !matches!(player.action, PlayerAction::None)
            || player.payoff.is_some()
        {
            let mut action_rect = rect.shrink(1.0,);
            action_rect.set_height(rect.height() / 2.0,);

            let rounding = CornerRadius {
                nw: 4,
                ne: 4,
                ..CornerRadius::default()
            };

            ui.painter().rect(
                action_rect,
                rounding,
                Self::TEXT_COLOR,
                Stroke::NONE,
                StrokeKind::Inside,
            );

            let label = if player.payoff.is_some() {
                "WINNER"
            } else {
                player.action.label()
            };

            ui.painter().text(
                rect.left_top() + vec2(5.0, 3.0,),
                Align2::LEFT_TOP,
                label,
                FontId::new(13.0, FontFamily::Monospace,),
                Self::BG_COLOR,
            );

            if player.bet > Chips::ZERO || player.payoff.is_some() {
                let amount_rect = action_rect
                    .translate(vec2(3.0, action_rect.height() + 2.0,),);

                let amount = if player.bet > Chips::ZERO {
                    player.bet.to_string()
                } else {
                    player
                        .payoff
                        .as_ref()
                        .map(|p| p.chips.to_string(),)
                        .unwrap_or_default()
                };

                let galley = ui.painter().layout_no_wrap(
                    amount,
                    FontId::new(13.0, FontFamily::Monospace,),
                    Self::TEXT_COLOR,
                );

                ui.painter().galley(
                    amount_rect.left_top(),
                    galley,
                    Self::TEXT_COLOR,
                );
            }
        }
    }

    fn paint_player_name_and_chips(
        &self,
        player: &PlayerPrivate,
        ui: &Ui,
        rect: &Rect,
    ) {
        let bg_rect = Rect::from_min_size(
            rect.left_bottom() + vec2(0.0, 10.0,),
            vec2(rect.width(), 40.0,),
        );

        paint_border(ui, &bg_rect,);

        let painter = ui.painter().with_clip_rect(bg_rect.shrink(3.0,),);

        let font = FontId::new(13.0, FontFamily::Monospace,);

        let galley = ui.painter().layout_no_wrap(
            player.nickname.to_string(),
            font.clone(),
            Self::TEXT_COLOR,
        );

        painter.galley(
            bg_rect.left_top() + vec2(5.0, 4.0,),
            galley.clone(),
            Self::TEXT_COLOR,
        );

        let chips_pos = bg_rect.left_top() + vec2(0.0, galley.size().y,);

        let galley = ui.painter().layout_no_wrap(
            player.chips.to_string(),
            font,
            Self::TEXT_COLOR,
        );

        painter.galley(chips_pos + vec2(5.0, 7.0,), galley, Self::TEXT_COLOR,);

        if player.has_button {
            let btn_pos = bg_rect.right_top() + vec2(-10.0, 10.0,);
            painter.circle(btn_pos, 6.0, Self::TEXT_COLOR, Stroke::NONE,);
        }

        if !player.is_active {
            fill_inactive(ui, &bg_rect,);
        }
    }

    // ---------- the oval-shaped table background -------------------
    fn paint_table(&self, ui: &Ui, rect: &Rect,) {
        fn paint_oval(ui: &Ui, rect: &Rect, fill_color: Color32,) {
            let radius = rect.height() / 2.0;
            ui.painter().add(epaint::CircleShape {
                center: rect.left_center() + vec2(radius, 0.0,),
                radius,
                fill: fill_color,
                stroke: Stroke::NONE,
            },);

            ui.painter().add(epaint::CircleShape {
                center: rect.right_center() - vec2(radius, 0.0,),
                radius,
                fill: fill_color,
                stroke: Stroke::NONE,
            },);

            ui.painter().rect(
                Rect::from_center_size(
                    rect.center(),
                    vec2(2.0f32.mul_add(-radius, rect.width()), rect.height(),),
                ),
                0.0,
                fill_color,
                Stroke::NONE,
                StrokeKind::Inside,
            );
        }

        // Outer pad border
        paint_oval(ui, rect, Color32::from_rgb(200, 160, 80,),);

        // Table pad
        let mut outer = Color32::from_rgb(90, 90, 105,);
        let inner = Color32::from_rgb(15, 15, 50,);
        for pad in (2..45).step_by(3,) {
            paint_oval(ui, &rect.shrink(pad as f32,), outer,);
            outer = outer.lerp_to_gamma(inner, 0.1,);
        }

        // Inner pad border
        paint_oval(ui, &rect.shrink(50.0,), Color32::from_rgb(200, 160, 80,),);

        // Outer table
        let mut outer = Color32::from_rgb(40, 110, 20,);
        let inner = Color32::from_rgb(50, 150, 10,);
        for pad in (52..162).step_by(5,) {
            paint_oval(ui, &rect.shrink(pad as f32,), outer,);
            outer = outer.lerp_to_gamma(inner, 0.1,);
        }

        // Cards board
        paint_oval(ui, &rect.shrink(162.0,), Color32::from_gray(160,),);
        paint_oval(ui, &rect.shrink(164.0,), inner,);

        if self.game_state.players().is_empty() {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Waiting for players…",
                FontId::new(28.0, FontFamily::Monospace,),
                Color32::from_gray(200,),
            );
        }
    }

    // ---------- community cards -----------------------------------
    fn paint_board(&self, ui: &Ui, rect: &Rect, app: &App,) {
        const CARD: Vec2 = vec2(38.0, 72.0,);
        const GAP: f32 = 5.0;

        let cards = &self.game_state.board;
        if cards.is_empty() {
            return;
        }

        let width = CARD
            .x
            .mul_add(cards.len() as f32, GAP * (cards.len() as f32 - 1.0),);
        let start = rect.center() - vec2(width / 2.0, CARD.y / 2.0 + 20.0,);
        let mut rp = Rect::from_min_size(start, CARD,);

        for c in cards {
            Image::new(&app.textures.card(*c,),)
                .corner_radius(4.0,)
                .paint_at(ui, rp,);
            rp = rp.translate(vec2(CARD.x + GAP, 0.0,),);
        }
    }

    // ---------- central pot value ---------------------------------
    fn paint_pot(&mut self, ui: &Ui, rect: &Rect,) {
        if self.game_state.pot_chips() == Chips::ZERO {
            return;
        }

        let txt = self.game_state.pot_chips().to_string();
        let gal = ui.painter().layout_no_wrap(txt, TEXT_FONT, TEXT_COLOR,);
        let size = vec2(gal.size().x + 20.0, 32.0,);
        let rp = Rect::from_min_size(
            rect.center() - size / 2.0 - vec2(0.0, 40.0,),
            size,
        );
        ui.painter().rect_filled(rp, 4.0, BG_COLOR,);
        ui.painter()
            .galley(rp.center() - gal.size() / 2.0, gal, TEXT_COLOR,);
    }

    // ---------- every seat / player box ---------------------------
    fn paint_players(&mut self, ui: &mut Ui, rect: &Rect, app: &App,) {
        let seats = match self.game_state.players().len() {
            1 => vec![Align2::CENTER_BOTTOM],
            2 => vec![Align2::CENTER_BOTTOM, Align2::CENTER_TOP],
            3 => {
                vec![Align2::CENTER_BOTTOM, Align2::LEFT_TOP, Align2::RIGHT_TOP]
            },
            4 => {
                vec![
                    Align2::CENTER_BOTTOM,
                    Align2::LEFT_CENTER,
                    Align2::CENTER_TOP,
                    Align2::RIGHT_CENTER,
                ]
            },
            _ => {
                vec![
                    Align2::CENTER_BOTTOM,
                    Align2::LEFT_BOTTOM,
                    Align2::LEFT_TOP,
                    Align2::CENTER_TOP,
                    Align2::RIGHT_TOP,
                    Align2::RIGHT_BOTTOM,
                ]
            },
        };

        for (p, a,) in self.game_state.players.iter().zip(seats.iter(),) {
            self.paint_single_player(p, a, ui, rect, app,);
        }

        self.paint_action_controls(ui, rect, app,);
    }

    // ---------- single player frame -------------------------------
    fn paint_single_player(
        &self,
        player: &PlayerPrivate,
        align: &Align2,
        ui: &Ui,
        rect: &Rect,
        app: &App,
    ) {
        let rect = player_rect(rect, *align,);
        let id_rect = self.paint_player_id(player, ui, &rect, align,);
        self.paint_player_name_and_chips(player, ui, &id_rect,);
        self.paint_player_cards(player, ui, &id_rect, *align, &app.textures,);
        self.paint_player_action(player, ui, &id_rect, align,);
        self.paint_winning_hand(player, ui, &id_rect, align, &app.textures,);
    }

    fn paint_winning_hand(
        &self,
        player: &PlayerPrivate,
        ui: &Ui,
        rect: &Rect,
        align: &Align2,
        textures: &Textures,
    ) {
        const IMAGE_LY: f32 = 60.0;
        const LABEL_LY: f32 = 20.0;

        if let Some(payoff,) = &player.payoff {
            if payoff.cards.is_empty() {
                return;
            }

            let x_pos = if align.x() == Align::RIGHT {
                rect.left_top().x - rect.size().x - 10.0
            } else {
                rect.left_top().x
            };

            let y_pos = if align.y() == Align::TOP {
                rect.left_top().y + 130.0
            } else {
                rect.left_top().y - (IMAGE_LY + LABEL_LY + 10.0)
            };

            let cards_rect = Rect::from_min_size(
                pos2(x_pos, y_pos,),
                vec2(
                    Self::ACTION_BUTTON_LX.mul_add(2.0, 10.0,),
                    IMAGE_LY + LABEL_LY,
                ),
            );

            paint_border(ui, &cards_rect,);

            let card_lx = (cards_rect.size().x - 11.0) / 5.0;
            let card_size = vec2(card_lx, IMAGE_LY - 8.0,);
            let mut single_card_rect = Rect::from_min_size(
                cards_rect.left_top() + vec2(4.0, 4.0,),
                card_size,
            );

            for card in &payoff.cards {
                let tx = textures.card(*card,);
                Image::new(&tx,)
                    .corner_radius(2.0,)
                    .paint_at(ui, single_card_rect,);

                single_card_rect =
                    single_card_rect.translate(vec2(card_lx + 1.0, 0.0,),);
            }

            let rank_rect = Rect::from_min_size(
                pos2(x_pos, y_pos + IMAGE_LY - 2.0,),
                vec2(cards_rect.width(), LABEL_LY,),
            );

            let rounding = CornerRadius {
                sw: 4,
                se: 4,
                ..CornerRadius::default()
            };

            ui.painter().rect(
                rank_rect.shrink2(vec2(2.0, 0.0,),),
                rounding,
                Self::TEXT_COLOR,
                Stroke::NONE,
                StrokeKind::Inside,
            );

            ui.painter().text(
                rank_rect.center(),
                Align2::CENTER_CENTER,
                &payoff.rank,
                FontId::new(14.0, FontFamily::Monospace,),
                Self::BG_COLOR,
            );
        }
    }

    fn paint_player_id(
        &self,
        player: &PlayerPrivate,
        ui: &Ui,
        rect: &Rect,
        align: &Align2,
    ) -> Rect {
        let rect = rect.shrink(5.0,);

        let layout_job = LayoutJob {
            wrap: text::TextWrapping::wrap_at_width(75.0,),
            ..LayoutJob::single_section(player.id_digits(), TextFormat {
                font_id: FontId::new(13.0, FontFamily::Monospace,),
                extra_letter_spacing: 1.0,
                color: Self::TEXT_COLOR,
                ..Default::default()
            },)
        };

        let galley = ui.painter().layout_job(layout_job,);

        let min_pos = if align.x() == Align::RIGHT {
            rect.right_top() - vec2(galley.size().x, 0.0,)
        } else {
            rect.left_top()
        };

        // Paint peer id rect.
        let rect = Rect::from_min_size(min_pos, galley.rect.size(),);

        let bg_rect = rect.expand(5.0,);
        paint_border(ui, &bg_rect,);

        if let Some(timer,) = player.action_timer {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                timer.to_string(),
                FontId::new(50.0, FontFamily::Monospace,),
                Self::TEXT_COLOR,
            );
        } else {
            let text_pos = rect.left_top();
            ui.painter().galley(text_pos, galley, Color32::DARK_GRAY,);
        }

        if !player.is_active {
            fill_inactive(ui, &bg_rect,);
        }

        bg_rect
    }

    // ---------- controls (fold, call, raise…) ---------------------
    fn paint_action_controls(&mut self, ui: &mut Ui, rect: &Rect, app: &App,) {
        let mut send_action = None;

        if let Some(req,) = self.game_state.action_req() {
            let rect = Self::player_rect(rect, &Align2::CENTER_BOTTOM,);

            let mut btn_rect = Rect::from_min_size(
                rect.left_top() + vec2(0.0, 130.0,),
                vec2(Self::ACTION_BUTTON_LX, Self::ACTION_BUTTON_LY,),
            );

            for action in &req.actions {
                Self::paint_border(ui, &btn_rect,);

                let label = match action {
                    PlayerAction::Bet { .. } | PlayerAction::Raise { .. }
                        if self.bet_params.is_some() =>
                    {
                        // Set the label for bet and raise to confirm if betting
                        // controls are active.
                        "CONFIRM"
                    },
                    _ => action.label(),
                };

                let btn = Button::new(
                    RichText::new(label,)
                        .font(Self::TEXT_FONT,)
                        .color(Self::TEXT_COLOR,),
                )
                .fill(Self::BG_COLOR,);

                let clicked = ui.put(btn_rect.shrink(2.0,), btn,).clicked();
                match action {
                    PlayerAction::Call | PlayerAction::Check => {
                        if ui.input(|i| i.key_pressed(Key::C,),) || clicked {
                            send_action = Some((*action, Chips::ZERO,),);
                            self.bet_params = None;
                            break;
                        }
                    },
                    PlayerAction::Fold => {
                        if ui.input(|i| i.key_pressed(Key::F,),) || clicked {
                            send_action = Some((*action, Chips::ZERO,),);
                            self.bet_params = None;
                            break;
                        }
                    },
                    PlayerAction::Bet { .. } | PlayerAction::Raise { .. } => {
                        if ui.input(|i| i.key_pressed(Key::Enter,),) || clicked
                        {
                            if let Some(params,) = &self.bet_params {
                                send_action = Some((
                                    *action,
                                    Chips::new(params.raise_value,),
                                ),);
                                self.bet_params = None;
                                break;
                            }
                        }

                        if (ui.input(|i| i.key_pressed(Key::B,),)
                            || ui.input(|i| i.key_pressed(Key::R,),)
                            || clicked)
                            && self.bet_params.is_none()
                        {
                            self.bet_params = Some(BetParams {
                                min_raise:   req.min_raise.into(),
                                big_blind:   req.big_blind.into(),
                                raise_value: req.min_raise.into(),
                            },);
                        }
                    },
                    _ => {},
                }

                btn_rect = btn_rect
                    .translate(vec2(Self::ACTION_BUTTON_LX + 10.0, 0.0,),);
            }

            self.paint_betting_controls(ui, &rect,);
        }

        if let Some((action, amount,),) = send_action {
            let msg = UIEvent::Action {
                kind: action,
                amount,
            };

            let _ = app.send_cmd_to_engine(msg,);
        }
    }

    // ---------- help / legend overlay -----------------------------
    fn paint_help_button(&mut self, ui: &mut Ui, rect: &Rect,) {
        let btn_rect = Rect::from_min_size(
            rect.right_top() - vec2(SMALL_BUTTON_SZ.x, 0.0,),
            SMALL_BUTTON_SZ,
        );
        if ui
            .put(
                btn_rect,
                Button::new(
                    RichText::new("?",).font(TEXT_FONT,).color(TEXT_COLOR,),
                )
                .fill(BG_COLOR,),
            )
            .clicked()
        {
            self.show_legend ^= true;
        }
    }

    fn paint_local_peer_id(&self, ui: &Ui, rect: &Rect,) {
        // build a `LayoutJob` manually instead of the old `.into_job()`
        let mut job = LayoutJob::default();
        let id: String = format!("id: {}", self.game_state.player_id());
        job.append(&id, 0.0, TextFormat {
            font_id: TEXT_FONT,
            color: TEXT_COLOR,
            ..Default::default()
        },);

        let gal = ui.painter().layout_job(job,);

        let pad = vec2(5.0, 5.0,);
        let x = rect.left() + pad.x;
        let y = 7.0f32.mul_add(pad.y, rect.top(),);
        let bg = Rect::from_min_size(Pos2::new(x, y,), gal.size() + pad,);
        ui.painter().rect_filled(bg, 4.0, BG_COLOR,);
        ui.painter().galley(bg.min + pad / 2.0, gal, TEXT_COLOR,);
    }

    fn paint_listen_addr(&self, ui: &Ui, rect: &Rect,) {
        // build a `LayoutJob` manually instead of the old `.into_job()`
        let mut job = LayoutJob::default();
        let mut id: String = "addr: None".to_string();
        if let Some(addr,) = self.game_state.listen_addr.clone() {
            id = format!("addr: {addr}");
        }
        job.append(&id, 0.0, TextFormat {
            font_id: TEXT_FONT,
            color: TEXT_COLOR,
            ..Default::default()
        },);

        let gal = ui.painter().layout_job(job,);

        let pad = vec2(5.0, 5.0,);
        // add 5 pixels of border to the right.
        let x = rect.right() - gal.size().x - pad.x;
        let y = rect.bottom() - gal.size().y - pad.y;
        let bg = Rect::from_min_size(Pos2::new(x, y,), gal.size() + pad,);
        ui.painter().rect_filled(bg, 4.0, BG_COLOR,);
        ui.painter().galley(bg.min + pad / 2.0, gal, TEXT_COLOR,);
    }

    fn paint_hand_phase(&self, ui: &Ui, rect: &Rect,) {
        // build a `LayoutJob` manually instead of the old `.into_job()`
        let mut job = LayoutJob::default();
        let id: String =
            format!("phase: {}", self.game_state.hand_phase.clone());
        job.append(&id, 0.0, TextFormat {
            font_id: TEXT_FONT,
            color: TEXT_COLOR,
            ..Default::default()
        },);

        let gal = ui.painter().layout_job(job,);

        let pad = vec2(5.0, 5.0,);
        let x = rect.left() + pad.x;
        let y = rect.top() + pad.y;
        let bg = Rect::from_min_size(Pos2::new(x, y,), gal.size() + pad,);
        ui.painter().rect_filled(bg, 4.0, BG_COLOR,);
        ui.painter().galley(bg.min + pad / 2.0, gal, TEXT_COLOR,);
    }

    fn paint_legend(&self, ui: &Ui, rect: &Rect,) {
        if !self.show_legend {
            return;
        }

        const HELP: &str = r"C  Call / Check
F  Fold
R  Raise
B  Bet
↑  +1 BB     PgUp  +4 BB
↓  −1 BB     PgDn  −4 BB
⏎  Confirm
?  Toggle help";

        // build a `LayoutJob` manually instead of the old `.into_job()`
        let mut job = LayoutJob::default();
        job.append(HELP, 0.0, TextFormat {
            font_id: TEXT_FONT,
            color: TEXT_COLOR,
            ..Default::default()
        },);

        let gal = ui.painter().layout_job(job,);

        let pad = vec2(10.0, 10.0,);
        let bg = Rect::from_min_size(
            rect.center() - gal.size() / 2.0,
            gal.size() + pad,
        );
        ui.painter().rect_filled(bg, 4.0, BG_COLOR,);
        ui.painter().galley(bg.min + pad / 2.0, gal, TEXT_COLOR,);
    }

    fn player_rect(rect: &Rect, align: &Align2,) -> Rect {
        const PLAYER_SIZE: Vec2 = vec2(120.0, 160.0,);

        let rect = rect.shrink(20.0,);
        let x = match align.x() {
            Align::LEFT => rect.left(),
            Align::Center => rect.center().x - PLAYER_SIZE.x / 1.5,
            Align::RIGHT => rect.right() - PLAYER_SIZE.x,
        };

        let y = match (align.x(), align.y(),) {
            (Align::LEFT | Align::RIGHT, Align::TOP,) => {
                rect.top() + rect.height() / 4.0 - PLAYER_SIZE.y / 2.0
            },
            (Align::LEFT | Align::RIGHT, Align::BOTTOM,) => {
                rect.bottom() - rect.height() / 4.0 - PLAYER_SIZE.y / 2.0
            },
            (Align::LEFT | Align::RIGHT, Align::Center,) => {
                rect.bottom() - rect.height() / 2.0 - PLAYER_SIZE.y / 2.0
            },
            (Align::Center, Align::TOP,) => rect.top(),
            (Align::Center, Align::BOTTOM,) => rect.bottom() - PLAYER_SIZE.y,
            _ => unreachable!(),
        };
        Rect::from_min_size(pos2(x, y,), PLAYER_SIZE,)
    }
    fn paint_border(ui: &Ui, rect: &Rect,) {
        let border_color = Color32::from_gray(20,);
        ui.painter().rect(
            *rect,
            5.0,
            border_color,
            Stroke::NONE,
            StrokeKind::Inside,
        );

        for (idx, &color,) in (0..6).zip(&[100, 120, 140, 100, 80,],) {
            let border_rect = rect.expand(idx as f32,);
            let stroke = Stroke::new(1.0, Color32::from_gray(color as u8,),);
            ui.painter().rect_stroke(
                border_rect,
                5.0,
                stroke,
                StrokeKind::Inside,
            );
        }
    }

    fn paint_betting_controls(&mut self, ui: &mut Ui, rect: &Rect,) {
        const TEXT_FONT: FontId = FontId::new(15.0, FontFamily::Monospace,);

        if let Some(params,) = self.bet_params.as_mut() {
            let rect = Rect::from_min_size(
                rect.left_top() + vec2(182.0, 0.0,),
                vec2(Self::ACTION_BUTTON_LX, 120.0,),
            );

            Self::paint_border(ui, &rect,);

            let mut ypos = 5.0;

            ui.painter().text(
                rect.left_top() + vec2(7.0, ypos,),
                Align2::LEFT_TOP,
                "Raise To",
                FontId::new(14.0, FontFamily::Monospace,),
                Self::TEXT_COLOR,
            );

            let galley = ui.painter().layout_no_wrap(
                Chips::new(params.raise_value,).to_string(),
                FontId::new(14.0, FontFamily::Monospace,),
                Self::TEXT_COLOR,
            );

            ypos += 35.0;
            ui.painter().galley(
                rect.left_top()
                    + vec2((rect.width() - galley.size().x) / 2.0, ypos,),
                galley,
                Self::TEXT_COLOR,
            );

            let big_blind = params.big_blind;

            // Maximum bet is the local player chips.
            let max_bet = self
                .game_state
                .players()
                .first()
                .map(|p| (p.chips + p.bet).into(),)
                .unwrap();

            // Handle case when minimum raise is greater than this player chips,
            // so that the player can go all in.
            let min_raise = params.min_raise.min(max_bet,);
            let slider =
                Slider::new(&mut params.raise_value, min_raise..=max_bet,)
                    .show_value(false,)
                    .step_by(f64::from(big_blind,),)
                    .trailing_fill(true,);

            ui.style_mut().spacing.slider_width = rect.width() - 10.0;
            ui.visuals_mut().selection.bg_fill = Self::TEXT_COLOR;

            ypos += 35.0;
            let slider_rect = Rect::from_min_size(
                rect.left_top() + vec2(5.0, ypos,),
                vec2(rect.width(), 20.0,),
            );
            ui.put(slider_rect, slider,);

            // Adjust slider value in case it goes above max_bet, this may
            // happen if the max_bet is not a multiple of the slider
            // step_by.
            params.raise_value = params.raise_value.min(max_bet,);

            ypos += 20.0;
            let btn = Button::new(
                RichText::new("-",)
                    .font(TEXT_FONT,)
                    .color(Self::TEXT_COLOR,),
            )
            .fill(Self::BG_COLOR,);
            let btn_rect = Rect::from_min_size(
                rect.left_top() + vec2(0.0, ypos,),
                vec2(rect.width() / 2.0 - 2.0, 20.0,),
            );

            // Button click, down arrow or left arrow subtracts 1 big blind.
            if ui.put(btn_rect, btn,).clicked()
                || ui.input(|i| i.key_pressed(Key::ArrowDown,),)
                || ui.input(|i| i.key_pressed(Key::ArrowLeft,),)
            {
                params.raise_value = params
                    .raise_value
                    .saturating_sub(big_blind,)
                    .max(min_raise,);
            }

            // Page down to subtract 4 big blinds
            if ui.input(|i| i.key_pressed(Key::PageDown,),) {
                params.raise_value = params
                    .raise_value
                    .saturating_sub(big_blind * 4,)
                    .max(min_raise,);
            }

            let btn = Button::new(
                RichText::new("+",)
                    .font(TEXT_FONT,)
                    .color(Self::TEXT_COLOR,),
            )
            .fill(Self::BG_COLOR,);
            let btn_rect = Rect::from_min_size(
                rect.left_top() + vec2(rect.width() / 2.0, ypos,),
                vec2(rect.width() / 2.0, 20.0,),
            );

            // Button click, up arrow or right arrow adds 1 big blind.
            if ui.put(btn_rect, btn,).clicked()
                || ui.input(|i| i.key_pressed(Key::ArrowUp,),)
                || ui.input(|i| i.key_pressed(Key::ArrowRight,),)
            {
                params.raise_value =
                    params.raise_value.saturating_add(big_blind,).min(max_bet,);
            }

            // Page up to add 4 big blinds
            if ui.input(|i| i.key_pressed(Key::PageUp,),) {
                params.raise_value = params
                    .raise_value
                    .saturating_add(big_blind * 4,)
                    .min(max_bet,);
            }
        }
    }
    fn paint_player_cards(
        &self,
        player: &PlayerPrivate,
        ui: &Ui,
        rect: &Rect,
        align: Align2,
        textures: &Textures,
    ) {
        if !player.is_active {
            return;
        }

        let (tx1, tx2,) = match player.public_cards {
            PlayerCards::None => return,
            PlayerCards::Covered => (textures.back(), textures.back(),),
            PlayerCards::Cards(c1, c2,) => {
                (textures.card(c1,), textures.card(c2,),)
            },
        };

        let cards_rect = if align.x() == Align::RIGHT {
            Rect::from_min_size(
                rect.left_top() - vec2(rect.size().x + 10.0, 0.0,),
                rect.size(),
            )
        } else {
            Rect::from_min_size(
                rect.right_top() + vec2(10.0, 0.0,),
                rect.size(),
            )
        };

        paint_border(ui, &cards_rect,);

        let card_lx = (rect.size().x - 10.0) / 2.0;
        let card_size = vec2(card_lx, rect.size().y - 8.0,);

        let card_pos = cards_rect.left_top() + vec2(4.0, 4.0,);
        let c1_rect = Rect::from_min_size(card_pos, card_size,);
        Image::new(&tx1,).corner_radius(2.0,).paint_at(ui, c1_rect,);

        let c2_rect = Rect::from_min_size(
            card_pos + vec2(card_size.x + 2.0, 0.0,),
            card_size,
        );
        Image::new(&tx2,).corner_radius(2.0,).paint_at(ui, c2_rect,);
    }
}

fn paint_border(ui: &Ui, rect: &Rect,) {
    let border_color = Color32::from_gray(20,);
    ui.painter().rect(
        *rect,
        5.0,
        border_color,
        Stroke::NONE,
        StrokeKind::Inside,
    );

    for (idx, &color,) in (0u8..6u8).zip(&[100u8, 120u8, 140u8, 100u8, 80u8,],)
    {
        let border_rect = rect.expand(f32::from(idx,),);
        let stroke = Stroke::new(1.0, Color32::from_gray(color,),);
        ui.painter()
            .rect_stroke(border_rect, 5.0, stroke, StrokeKind::Inside,);
    }
}
fn player_rect(rect: &Rect, align: Align2,) -> Rect {
    const PLAYER_SIZE: Vec2 = vec2(120.0, 160.0,);

    let rect = rect.shrink(20.0,);
    let x = match align.x() {
        Align::LEFT => rect.left(),
        Align::Center => rect.center().x - PLAYER_SIZE.x / 1.5,
        Align::RIGHT => rect.right() - PLAYER_SIZE.x,
    };

    let y = match (align.x(), align.y(),) {
        (Align::LEFT | Align::RIGHT, Align::TOP,) => {
            rect.top() + rect.height() / 4.0 - PLAYER_SIZE.y / 2.0
        },
        (Align::LEFT | Align::RIGHT, Align::BOTTOM,) => {
            rect.bottom() - rect.height() / 4.0 - PLAYER_SIZE.y / 2.0
        },
        (Align::LEFT | Align::RIGHT, Align::Center,) => {
            rect.bottom() - rect.height() / 2.0 - PLAYER_SIZE.y / 2.0
        },
        (Align::Center, Align::TOP,) => rect.top(),
        (Align::Center, Align::BOTTOM,) => rect.bottom() - PLAYER_SIZE.y,
        _ => unreachable!(),
    };

    Rect::from_min_size(pos2(x, y,), PLAYER_SIZE,)
}

fn fill_inactive(ui: &Ui, rect: &Rect,) {
    ui.painter().rect(
        *rect,
        2.0,
        Color32::from_rgba_unmultiplied(60, 60, 60, 140,),
        Stroke::NONE,
        StrokeKind::Inside,
    );
}
