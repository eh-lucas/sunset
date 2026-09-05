//! Carregamento da foto e geração do preview reduzido.

use image::RgbImage;
use std::path::{Path, PathBuf};

/// Maior lado do preview. Os ajustes rodam sobre esta versão enquanto o slider
/// se mexe; a resolução cheia só é tocada na exportação.
const PREVIEW_MAX: u32 = 1600;

pub struct Photo {
    pub path: PathBuf,
    pub full: RgbImage,
    pub preview: RgbImage,
}

impl Photo {
    pub fn open(path: &Path) -> Result<Self, String> {
        let full = image::open(path)
            .map_err(|e| format!("não consegui abrir: {e}"))?
            .to_rgb8();

        let (w, h) = full.dimensions();
        if w == 0 || h == 0 {
            return Err("imagem vazia".into());
        }

        let preview = if w.max(h) > PREVIEW_MAX {
            let scale = PREVIEW_MAX as f32 / w.max(h) as f32;
            image::imageops::resize(
                &full,
                (w as f32 * scale).round().max(1.0) as u32,
                (h as f32 * scale).round().max(1.0) as u32,
                image::imageops::FilterType::Triangle,
            )
        } else {
            full.clone()
        };

        Ok(Self {
            path: path.to_path_buf(),
            full,
            preview,
        })
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sem nome".into())
    }
}
