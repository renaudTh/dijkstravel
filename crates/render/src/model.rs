//! Entrée du moteur, écrite par `src/render/native.ts` : déjà validée et complétée côté Node
//! (`parseRun`, `parseTheme`, `parseTour`), elle est lue telle quelle.

use serde::Deserialize;
use std::collections::HashMap;

pub use dijkstravel_core::theme::{BasemapTheme, CameraTheme, HudTheme, RouteTheme, StopsTheme, Theme};
pub use dijkstravel_core::tour::{Stop, Tour};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input {
    pub video: Video,
    pub theme: Theme,
    /// `tour.json` tel que l'écrit le solveur : lu directement, sans recopie dans l'entrée.
    pub tour_file: String,
    #[serde(default)]
    pub title: String,
    /// Image de chaque lieu qui en a une : identifiant du lieu → fichier.
    #[serde(default)]
    pub images: HashMap<String, String>,
    pub imagery: ImagerySource,
    /// Relief (Terrarium) d'où vient l'ombrage.
    pub terrain: TileSource,
    /// Tuiles vectorielles d'où viennent les frontières.
    pub vector: TileSource,
    /// Cache disque des tuiles (`data/cache/tiles`), rempli par Node avant le rendu.
    pub tile_dir: String,
    /// Police de la carte d'infos, un fichier TTF par graisse.
    #[serde(default)]
    pub hud_fonts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagerySource {
    pub name: String,
    pub maxzoom: u8,
    pub credit: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TileSource {
    pub name: String,
    pub maxzoom: u8,
}

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
