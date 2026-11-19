//! The look of `nomad/gui/knob.py` and friends: dark green on near black, flat,
//! small type (Tk 8 pt ~ 11 px), no hover animations.

use egui::{Color32, FontId, Stroke, TextStyle, Visuals};

pub const BG: Color32 = Color32::from_rgb(0x10, 0x13, 0x10);
pub const GREEN: Color32 = Color32::from_rgb(0x55, 0xe6, 0x9a);
pub const GREY: Color32 = Color32::from_rgb(0x99, 0x99, 0x99);
pub const TROUGH: Color32 = Color32::from_rgb(0x29, 0x34, 0x2b);
pub const ACCENT: Color32 = Color32::from_rgb(0xb9, 0xff, 0xd8);
pub const PLAYHEAD: Color32 = Color32::WHITE;

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = BG;
    v.extreme_bg_color = BG;
    v.faint_bg_color = TROUGH;
    v.override_text_color = Some(GREY);
    v.selection.bg_fill = TROUGH;
    v.selection.stroke = Stroke::new(1.0, GREEN);
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.bg_fill = TROUGH;
        w.weak_bg_fill = TROUGH;
        w.bg_stroke = Stroke::NONE;
        w.fg_stroke = Stroke::new(1.0, GREY);
        w.corner_radius = 0.into();
        w.expansion = 0.0;
    }
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, GREEN);
    v.widgets.active.fg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.noninteractive.bg_fill = BG;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, GREY);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, TROUGH);
    v.widgets.noninteractive.corner_radius = 0.into();
    v.window_corner_radius = 0.into();
    v.menu_corner_radius = 0.into();
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.animation_time = 0.0;
        s.text_styles = [
            (TextStyle::Heading, FontId::proportional(15.0)),
            (TextStyle::Body, FontId::proportional(11.0)),
            (TextStyle::Button, FontId::proportional(11.0)),
            (TextStyle::Small, FontId::proportional(9.0)),
            (TextStyle::Monospace, FontId::monospace(11.0)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(4.0, 3.0);
        s.spacing.button_padding = egui::vec2(6.0, 2.0);
    });
}

/// Symbols of the Python UI, mapped to glyphs egui's bundled fonts draw
/// (`monochrome_emoji_fonts`): ▾ -> ⏷, ▸ -> ⏵, ⇄ -> ↔, ● -> ⚫; · and × as is.
pub mod sym {
    pub const DROPDOWN: &str = "⏷";
    pub const EXPAND: &str = "⏵";
    pub const FOLLOWS: &str = "↔";
    pub const MASTER: &str = "⚫";
    pub const DOT: &str = "·";
    pub const CLOSE: &str = "×";
    pub const ALL: [&str; 6] = [DROPDOWN, EXPAND, FOLLOWS, MASTER, DOT, CLOSE];
}
