//! Entrée du moteur, écrite par `src/render/native.ts` : déjà validée et complétée côté Node
//! (`parseRun`, `parseTheme`, `parseTour`), elle est lue telle quelle.

use serde::Deserialize;
use std::collections::HashMap;

use crate::geo::{LonLat, MeasuredPath};

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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stop {
    pub id: String,
    pub name: String,
    pub at: LonLat,
    /// Distance le long de `Tour::path` à l'arrivée sur ce lieu.
    pub meters: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tour {
    pub path: MeasuredPath,
    pub stops: Vec<Stop>,
    /// Traversées en bateau le long de `path`, en mètres `[début, fin]`, dans l'ordre.
    #[serde(default)]
    pub ferries: Vec<[f64; 2]>,
}

/// Miroir de `Theme` (`src/theme.ts`), complet : les valeurs par défaut sont posées par Node.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    pub route: RouteTheme,
    pub stops: StopsTheme,
    pub basemap: BasemapTheme,
    pub hud: HudTheme,
    pub camera: CameraTheme,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTheme {
    pub color: String,
    pub core_color: String,
    pub width: f64,
    pub core_ratio: f64,
    pub core_blur_ratio: f64,
    pub stripes: bool,
    pub stripe_color: String,
    pub ferry_color: String,
    pub head_radius: f64,
    pub head_color: String,
    pub head_icon: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopsTheme {
    pub unvisited_color: String,
    pub unvisited_radius: f64,
    pub visited_ratio: f64,
    pub pop_radius: f64,
    pub icon: String,
    pub icon_size: f64,
    pub show_image_on_map: bool,
    pub image_size: f64,
    pub show_labels: bool,
    pub label_size: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BasemapTheme {
    pub imagery: String,
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    pub hillshade: f64,
    pub borders: bool,
    pub border_color: String,
    pub tint_color: String,
    pub tint_opacity: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HudTheme {
    pub font: String,
    pub size: f64,
    pub background: String,
    pub text_color: String,
    pub title_color: String,
    pub distance_color: String,
    pub pill_from: String,
    pub pill_to: String,
    pub pill_text_color: String,
    pub show_title: bool,
    pub show_count: bool,
    pub show_last_stop: bool,
    pub show_image: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraTheme {
    pub follow_fraction: f64,
    pub stop_slowdown: f64,
}
