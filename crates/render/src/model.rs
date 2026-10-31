//! Entrée du moteur, construite par `scene::Scene` à partir du run, du thème et de la
//! tournée déjà validés.

use serde::Deserialize;
use std::collections::HashMap;

pub use dijkstravel_core::theme::{BasemapTheme, CameraTheme, HudTheme, RouteTheme, StopsTheme, Theme};
pub use dijkstravel_core::tour::{Stop, Tour};

#[derive(Debug, Clone)]
pub struct Input {
    pub video: Video,
    pub theme: Theme,
    pub title: String,
    /// Image de chaque lieu qui en a une : identifiant du lieu → fichier.
    pub images: HashMap<String, String>,
    pub imagery: ImagerySource,
    /// Sous `imagery`, visible là où elle n'a rien (`basemap.seaImagery`).
    pub sea_imagery: Option<ImagerySource>,
    /// Fond des transferts entre étapes (`basemap.transferImagery`), en fondu depuis `imagery`.
    pub transfer_imagery: Option<ImagerySource>,
    /// Relief (Terrarium) d'où vient l'ombrage.
    pub terrain: TileSource,
    /// Tuiles vectorielles d'où viennent les frontières.
    pub vector: TileSource,
    /// Cache disque des tuiles (`data/cache/tiles`), rempli avant le rendu (`download`).
    pub tile_dir: String,
    /// Police de la carte d'infos, un fichier TTF par graisse.
    pub hud_fonts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImagerySource {
    pub name: String,
    pub maxzoom: u8,
    pub credit: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TileSource {
    pub name: String,
    pub maxzoom: u8,
}

/// Petit côté de l'écran de référence, en pixels : le cadrage et les tailles du thème y sont réglés, et
/// tout se met à l'échelle d'une autre taille (`Video::scale`).
pub const REFERENCE_SIDE: f64 = 1080.0;

/// Lue aussi dans les valeurs de référence des tests.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Video {
    pub width: u32,
    pub height: u32,
    pub duration_sec: f64,
    pub intro_sec: f64,
    pub outro_sec: f64,
    pub fps: u32,
}

impl Video {
    /// Pixels de l'image par pixel de l'écran de référence : 2 en 2160 × 3840.
    pub fn scale(&self) -> f64 {
        f64::from(self.width.min(self.height)) / REFERENCE_SIDE
    }

    /// Taille de l'écran de référence de même format, sur lequel se placent le tracé, les lieux et la
    /// carte d'infos.
    pub fn reference_size(&self) -> (u32, u32) {
        let scale = self.scale();
        ((f64::from(self.width) / scale).round() as u32, (f64::from(self.height) / scale).round() as u32)
    }
}
