//! Lê a paleta do tema atual do Omarchy e aplica no egui.
//!
//! O Omarchy publica uma paleta semântica em
//! `~/.local/state/omarchy/current/theme/colors.toml`, então não precisamos
//! adivinhar cores a partir do tema do terminal.

use egui::Color32;

pub struct Palette {
    pub dark: bool,
    pub accent: Color32,
    pub selection: Color32,
    pub muted: Color32,
    pub background: Color32,
    pub dark_background: Color32,
    pub darker_background: Color32,
    pub lighter_background: Color32,
    pub foreground: Color32,
    pub dark_foreground: Color32,
    pub bright_foreground: Color32,
    pub red: Color32,
}

impl Default for Palette {
    /// Fallback quando não estamos num Omarchy (ou o tema sumiu): Tokyo Night.
    fn default() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(0x7a, 0xa2, 0xf7),
            selection: Color32::from_rgb(0x29, 0x2e, 0x42),
            muted: Color32::from_rgb(0x41, 0x48, 0x68),
            background: Color32::from_rgb(0x1a, 0x1b, 0x26),
            dark_background: Color32::from_rgb(0x13, 0x14, 0x1c),
            darker_background: Color32::from_rgb(0x0e, 0x0e, 0x14),
            lighter_background: Color32::from_rgb(0x24, 0x28, 0x3b),
            foreground: Color32::from_rgb(0xa9, 0xb1, 0xd6),
            dark_foreground: Color32::from_rgb(0x56, 0x5f, 0x89),
            bright_foreground: Color32::from_rgb(0xc0, 0xca, 0xf5),
            red: Color32::from_rgb(0xf7, 0x76, 0x8e),
        }
    }
}

fn theme_path() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        std::path::PathBuf::from(home)
            .join(".local/state/omarchy/current/theme/colors.toml"),
    )
}

fn hex(v: Option<&toml::Value>) -> Option<Color32> {
    let s = v?.as_str()?.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(s, 16).ok()?;
    Some(Color32::from_rgb(
        (n >> 16) as u8,
        (n >> 8) as u8,
        n as u8,
    ))
}

impl Palette {
    /// Carrega o tema atual do Omarchy, caindo no default se algo faltar.
    pub fn load() -> Self {
        let mut p = Palette::default();
        let Some(path) = theme_path() else { return p };
        let Ok(text) = std::fs::read_to_string(path) else { return p };
        let Ok(t) = text.parse::<toml::Table>() else { return p };

        if let Some(mode) = t.get("mode").and_then(|v| v.as_str()) {
            p.dark = mode != "light";
        }
        let set = |key: &str, slot: &mut Color32| {
            if let Some(c) = hex(t.get(key)) {
                *slot = c;
            }
        };
        set("accent", &mut p.accent);
        set("selection", &mut p.selection);
        set("muted", &mut p.muted);
        set("background", &mut p.background);
        set("dark_background", &mut p.dark_background);
        set("darker_background", &mut p.darker_background);
        set("lighter_background", &mut p.lighter_background);
        set("foreground", &mut p.foreground);
        set("dark_foreground", &mut p.dark_foreground);
        set("bright_foreground", &mut p.bright_foreground);
        set("red", &mut p.red);
        p
    }

    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        visuals.panel_fill = self.background;
        visuals.window_fill = self.background;
        visuals.extreme_bg_color = self.darker_background;
        visuals.faint_bg_color = self.lighter_background;
        visuals.override_text_color = Some(self.foreground);
        visuals.hyperlink_color = self.accent;
        visuals.selection.bg_fill = self.accent.gamma_multiply(0.4);
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, self.bright_foreground);
        visuals.window_stroke = egui::Stroke::new(1.0_f32, self.muted);

        let w = &mut visuals.widgets;
        w.noninteractive.bg_fill = self.background;
        w.noninteractive.weak_bg_fill = self.background;
        w.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, self.selection);
        w.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, self.dark_foreground);

        w.inactive.bg_fill = self.lighter_background;
        w.inactive.weak_bg_fill = self.lighter_background;
        w.inactive.bg_stroke = egui::Stroke::NONE;
        w.inactive.fg_stroke = egui::Stroke::new(1.0_f32, self.foreground);

        w.hovered.bg_fill = self.selection;
        w.hovered.weak_bg_fill = self.selection;
        w.hovered.bg_stroke = egui::Stroke::new(1.0_f32, self.muted);
        w.hovered.fg_stroke = egui::Stroke::new(1.5_f32, self.bright_foreground);

        w.active.bg_fill = self.accent;
        w.active.weak_bg_fill = self.accent;
        w.active.bg_stroke = egui::Stroke::new(1.0_f32, self.accent);
        w.active.fg_stroke = egui::Stroke::new(1.5_f32, self.bright_foreground);

        w.open.bg_fill = self.lighter_background;
        w.open.weak_bg_fill = self.lighter_background;

        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.spacing.slider_width = 190.0;
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.visuals.window_corner_radius = egui::CornerRadius::same(8);
        ctx.set_style(style);
    }
}
