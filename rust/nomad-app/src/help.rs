//! Help mode (`toggle_help`, `_help_enter`, `_help_show` of `nomad/gui/app.py`)
//! with the texts of `nomad/gui/help.py`, embedded from the reference export.
//!
//! Every control registers its rect and help key each frame (`reg`). With help
//! on, a full-window overlay takes the pointer, so no control reacts; the key
//! under the pointer is the smallest registered rect that contains it (Python
//! walks up to the nearest registered parent), and its text appears after
//! `HELP_DELAY_MS` beside it and stays until another control is shown.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use egui::{Color32, CursorIcon, Id, Order, Pos2, Rect, RichText, Sense};

use crate::theme::{BG, GREEN};

pub const HELP_DELAY_MS: u64 = 150;
pub const HELP_WRAP: f32 = 330.0;
pub const STATUS: &str = "AYUDA: pasá el mouse por un control (F1 / Esc para salir)";
const BODY: Color32 = Color32::from_rgb(0xdd, 0xdd, 0xdd);

fn texts() -> &'static HashMap<String, (String, String)> {
    static T: OnceLock<HashMap<String, (String, String)>> = OnceLock::new();
    T.get_or_init(|| {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../../reference/help.json")).expect("help.json");
        data["texts"]
            .as_object()
            .expect("texts")
            .iter()
            .map(|(k, v)| {
                let s = |i: usize| v[i].as_str().unwrap_or_default().to_string();
                (k.clone(), (s(0), s(1)))
            })
            .collect()
    })
}

/// `(title, body)` for a parameter name or panel element.
pub fn help_for(key: &str) -> Option<(&'static str, &'static str)> {
    texts().get(key).map(|(t, b)| (t.as_str(), b.as_str()))
}

#[derive(Default, Debug)]
pub struct HelpState {
    pub on: bool,
    hover: Option<(String, Instant)>,
    shown: Option<String>,
}

impl HelpState {
    /// On/off; turning off forgets what was shown or pending.
    pub fn toggle(&mut self) {
        self.on = !self.on;
        self.hover = None;
        self.shown = None;
    }

    /// The pointer is over `key`: show it after the delay (unless already shown).
    pub fn hover(&mut self, key: &str, now: Instant) {
        if self.shown.as_deref() == Some(key) || self.hover.as_ref().is_some_and(|(k, _)| k == key)
        {
            return;
        }
        self.hover = Some((key.to_string(), now));
    }

    /// The key whose text is visible now.
    pub fn visible(&mut self, now: Instant) -> Option<&str> {
        if let Some((k, t)) = &self.hover
            && now.saturating_duration_since(*t) >= Duration::from_millis(HELP_DELAY_MS)
        {
            self.shown = Some(k.clone());
            self.hover = None;
        }
        self.shown.as_deref()
    }
}

fn reg_id() -> Id {
    Id::unique("nomad-help-registry")
}

/// Register `r`'s (clipped) rect under `key`; returns `r` so calls can wrap widgets.
pub fn reg(r: egui::Response, key: &str) -> egui::Response {
    reg_rect(&r.ctx, r.interact_rect, key);
    r
}

pub fn reg_rect(ctx: &egui::Context, rect: Rect, key: &str) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Vec<(Rect, String)>>(reg_id())
            .push((rect, key.to_string()));
    });
}

/// Start of a frame: forget last frame's controls.
pub fn clear(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<Vec<(Rect, String)>>(reg_id()));
}

/// Everything registered so far this frame.
pub fn registered(ctx: &egui::Context) -> Vec<(Rect, String)> {
    ctx.data(|d| d.get_temp::<Vec<(Rect, String)>>(reg_id()))
        .unwrap_or_default()
}

/// The most specific registered control under `pos`.
pub fn key_at(regs: &[(Rect, String)], pos: Pos2) -> Option<(Rect, &str)> {
    regs.iter()
        .filter(|(r, _)| r.contains(pos))
        .min_by(|a, b| a.0.area().total_cmp(&b.0.area()))
        .map(|(r, k)| (*r, k.as_str()))
}

/// With help on (end of the frame): block the controls, follow the pointer and
/// draw the text. Returns true when the click landed on the `?` button (turn off).
pub fn help_ui(ctx: &egui::Context, state: &mut HelpState, now: Instant) -> bool {
    let regs = registered(ctx);
    let screen = ctx.content_rect();
    let mut off = false;
    egui::Area::new(Id::unique("nomad-help-overlay"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let r = ui.allocate_rect(screen, Sense::click_and_drag());
            ctx.set_cursor_icon(CursorIcon::Help);
            if let Some(pos) = r.hover_pos() {
                match key_at(&regs, pos) {
                    // The `?` button stays a button (Python leaves it out of help mode).
                    Some((_, "help")) => off = r.clicked(),
                    Some((_, k)) => state.hover(k, now),
                    None => {}
                }
            }
        });
    let shown = state.visible(now).map(str::to_string);
    if let Some(key) = shown
        && let Some((title, body)) = help_for(&key)
    {
        // Beside the control, or on its other side if it does not fit.
        let anchor = regs.iter().rev().find(|(_, k)| *k == key).map(|(r, _)| *r);
        let anchor = anchor.unwrap_or(Rect::from_min_size(screen.center(), egui::Vec2::ZERO));
        let width = HELP_WRAP + 24.0;
        let x = if anchor.right() + 8.0 + width <= screen.right() {
            anchor.right() + 8.0
        } else {
            (anchor.left() - width - 8.0).max(screen.left())
        };
        egui::Area::new(Id::unique("nomad-help-box"))
            .order(Order::Tooltip)
            .fixed_pos(Pos2::new(x, anchor.top().max(screen.top())))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(BG)
                    .stroke(egui::Stroke::new(1.0, GREEN))
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.set_max_width(HELP_WRAP);
                        ui.label(RichText::new(title).color(GREEN).strong().size(13.0));
                        ui.add_space(4.0);
                        ui.label(RichText::new(body).color(BODY).size(11.0));
                    });
            });
    }
    off
}
