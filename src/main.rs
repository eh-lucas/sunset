mod photo;
mod theme;
mod tone;

use eframe::egui;
use photo::Photo;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tone::Adjustments;

fn main() -> eframe::Result<()> {
    let initial = std::env::args_os().nth(1).map(PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([840.0, 560.0])
            .with_title("sunset")
            // app_id fixo para o Hyprland conseguir aplicar window rules.
            .with_app_id("sunset"),
        ..Default::default()
    };

    eframe::run_native(
        "sunset",
        options,
        Box::new(move |cc| Ok(Box::new(Sunset::new(cc, initial.clone())))),
    )
}

/// Resultado de uma exportação rodando em background.
type ExportSlot = Arc<Mutex<Option<Result<String, String>>>>;

struct Sunset {
    palette: theme::Palette,
    photo: Option<Photo>,
    adjustments: Adjustments,
    /// Textura do preview já com os ajustes aplicados.
    edited: Option<egui::TextureHandle>,
    /// Textura do preview original, para a comparação antes/depois.
    original: Option<egui::TextureHandle>,
    show_original: bool,
    status: String,
    exporting: bool,
    export_slot: ExportSlot,
}

impl Sunset {
    fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        let palette = theme::Palette::load();
        palette.apply(&cc.egui_ctx);

        let mut app = Self {
            palette,
            photo: None,
            adjustments: Adjustments::default(),
            edited: None,
            original: None,
            show_original: false,
            status: "Abra uma foto (Ctrl+O) ou arraste um arquivo para cá.".into(),
            exporting: false,
            export_slot: Arc::new(Mutex::new(None)),
        };

        if let Some(path) = initial {
            app.load(&cc.egui_ctx, path);
        }
        app
    }

    fn load(&mut self, ctx: &egui::Context, path: PathBuf) {
        match Photo::open(&path) {
            Ok(p) => {
                let (w, h) = p.full.dimensions();
                self.status = format!("{} · {w}×{h}", p.file_name());
                self.original = Some(ctx.load_texture(
                    "original",
                    to_color_image(&p.preview),
                    egui::TextureOptions::LINEAR,
                ));
                self.photo = Some(p);
                self.adjustments = Adjustments::default();
                self.edited = None;
                self.refresh_preview(ctx);
            }
            Err(e) => self.status = format!("{}: {e}", path.display()),
        }
    }

    /// Reaplica a curva atual sobre o preview e sobe a textura.
    fn refresh_preview(&mut self, ctx: &egui::Context) {
        let Some(photo) = &self.photo else { return };

        let pixels = if self.adjustments.is_neutral() {
            photo.preview.as_raw().clone()
        } else {
            tone::apply(photo.preview.as_raw(), &tone::build_lut(&self.adjustments))
        };

        let (w, h) = photo.preview.dimensions();
        let image = egui::ColorImage::from_rgb([w as usize, h as usize], &pixels);

        match &mut self.edited {
            Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
            None => {
                self.edited =
                    Some(ctx.load_texture("edited", image, egui::TextureOptions::LINEAR))
            }
        }
    }

    fn pick_and_load(&mut self, ctx: &egui::Context) {
        let picked = rfd::FileDialog::new()
            .add_filter("Imagens", &["jpg", "jpeg", "png", "JPG", "JPEG", "PNG"])
            .set_title("Abrir foto")
            .pick_file();
        if let Some(path) = picked {
            self.load(ctx, path);
        }
    }

    fn export(&mut self) {
        let Some(photo) = &self.photo else { return };
        if self.exporting {
            return;
        }

        let stem = photo
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "export".into());

        let dest = rfd::FileDialog::new()
            .set_title("Exportar")
            .set_file_name(format!("{stem}-sunset.jpg"))
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("PNG", &["png"])
            .save_file();
        let Some(dest) = dest else { return };

        // A resolução cheia só é processada aqui, numa thread, para o slider
        // nunca esperar por uma imagem de 50 megapixels.
        let source = photo.full.clone();
        let lut = tone::build_lut(&self.adjustments);
        let slot = Arc::clone(&self.export_slot);
        self.exporting = true;
        self.status = format!("exportando para {}…", dest.display());

        std::thread::spawn(move || {
            let (w, h) = source.dimensions();
            let pixels = tone::apply(source.as_raw(), &lut);
            let result = match image::RgbImage::from_raw(w, h, pixels) {
                Some(out) => out
                    .save(&dest)
                    .map(|_| format!("exportado: {}", dest.display()))
                    .map_err(|e| format!("falha ao salvar: {e}")),
                None => Err("buffer inválido na exportação".into()),
            };
            *slot.lock().unwrap() = Some(result);
        });
    }

    fn reset(&mut self, ctx: &egui::Context) {
        self.adjustments = Adjustments::default();
        self.refresh_preview(ctx);
    }
}

fn to_color_image(img: &image::RgbImage) -> egui::ColorImage {
    let (w, h) = img.dimensions();
    egui::ColorImage::from_rgb([w as usize, h as usize], img.as_raw())
}

/// Um slider do painel Básico, com botão de zerar só aquele ajuste.
fn tone_slider(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    decimals: usize,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(palette.foreground));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let modified = *value != 0.0;
            if ui
                .add_enabled(modified, egui::Button::new("↺").frame(false))
                .on_hover_text("zerar")
                .clicked()
            {
                *value = 0.0;
                changed = true;
            }
            ui.label(
                egui::RichText::new(format!("{:+.*}", decimals, value))
                    .monospace()
                    .color(if modified {
                        palette.accent
                    } else {
                        palette.dark_foreground
                    }),
            );
        });
    });
    changed |= ui
        .add(egui::Slider::new(value, range).show_value(false))
        .changed();
    ui.add_space(6.0);
    changed
}

impl eframe::App for Sunset {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(result) = self.export_slot.lock().unwrap().take() {
            self.exporting = false;
            self.status = match result {
                Ok(msg) => msg,
                Err(e) => e,
            };
        }

        // Arrastar e soltar um arquivo na janela abre a foto.
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(path) = dropped.into_iter().find_map(|f| f.path) {
            self.load(ctx, path);
        }

        let (open, export, reset) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::O),
                i.modifiers.command && i.key_pressed(egui::Key::S),
                i.modifiers.command && i.key_pressed(egui::Key::R),
            )
        });
        // Segurar a crase mostra a foto original, como o \ do Lightroom.
        self.show_original = ctx.input(|i| i.key_down(egui::Key::Backtick));

        if open {
            self.pick_and_load(ctx);
        }
        if export {
            self.export();
        }
        if reset {
            self.reset(ctx);
        }

        egui::TopBottomPanel::top("topo")
            .frame(
                egui::Frame::new()
                    .fill(self.palette.dark_background)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("sunset")
                            .color(self.palette.accent)
                            .strong(),
                    );
                    ui.add_space(12.0);
                    if ui.button("Abrir").clicked() {
                        self.pick_and_load(ctx);
                    }
                    let has_photo = self.photo.is_some();
                    if ui
                        .add_enabled(
                            has_photo && !self.exporting,
                            egui::Button::new("Exportar"),
                        )
                        .clicked()
                    {
                        self.export();
                    }
                    if ui
                        .add_enabled(
                            has_photo && !self.adjustments.is_neutral(),
                            egui::Button::new("Resetar"),
                        )
                        .clicked()
                    {
                        self.reset(ctx);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(&self.status)
                                .color(self.palette.dark_foreground)
                                .small(),
                        );
                    });
                });
            });

        egui::SidePanel::right("ajustes")
            .exact_width(272.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(self.palette.dark_background)
                    .inner_margin(egui::Margin::same(14)),
            )
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("BÁSICO")
                        .color(self.palette.dark_foreground)
                        .small()
                        .strong(),
                );
                ui.add_space(10.0);

                ui.add_enabled_ui(self.photo.is_some(), |ui| {
                    let a = &mut self.adjustments;
                    let p = &self.palette;
                    let mut changed = false;
                    changed |= tone_slider(ui, p, "Exposição", &mut a.exposure, -5.0..=5.0, 2);
                    changed |= tone_slider(ui, p, "Contraste", &mut a.contrast, -100.0..=100.0, 0);
                    ui.add_space(4.0);
                    changed |= tone_slider(ui, p, "Realces", &mut a.highlights, -100.0..=100.0, 0);
                    changed |= tone_slider(ui, p, "Sombras", &mut a.shadows, -100.0..=100.0, 0);
                    ui.add_space(4.0);
                    changed |= tone_slider(ui, p, "Brancos", &mut a.whites, -100.0..=100.0, 0);
                    changed |= tone_slider(ui, p, "Pretos", &mut a.blacks, -100.0..=100.0, 0);

                    if changed {
                        self.refresh_preview(ui.ctx());
                    }
                });

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("Ctrl+O abrir · Ctrl+S exportar · ` comparar")
                            .color(self.palette.muted)
                            .small(),
                    );
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(self.palette.darker_background)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                let texture = if self.show_original {
                    self.original.as_ref()
                } else {
                    self.edited.as_ref().or(self.original.as_ref())
                };

                let Some(texture) = texture else {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            egui::RichText::new("arraste uma foto para cá")
                                .color(self.palette.muted),
                        );
                    });
                    return;
                };

                let available = ui.available_size();
                let size = texture.size_vec2();
                let scale = (available.x / size.x).min(available.y / size.y).min(4.0);
                let shown = size * scale;

                ui.centered_and_justified(|ui| {
                    ui.add(
                        egui::Image::new(egui::load::SizedTexture::new(texture.id(), shown))
                            .corner_radius(2.0),
                    );
                });

                if self.show_original {
                    ui.painter().text(
                        ui.max_rect().left_top() + egui::vec2(6.0, 6.0),
                        egui::Align2::LEFT_TOP,
                        "ORIGINAL",
                        egui::FontId::proportional(12.0),
                        self.palette.accent,
                    );
                }
            });
    }
}
