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
    /// Heures du départ et de l'arrivée : en relief éclairé, le soleil et la lune de ces heures-là.
    pub clock: Option<Clock>,
}

/// Heures de passage le long du tracé : `[mètres, secondes UTC depuis 1970]`, croissants. Entre deux
/// passages, l'heure suit les mètres parcourus à allure constante.
#[derive(Debug, Clone, PartialEq)]
pub struct Clock {
    marks: Vec<[f64; 2]>,
}

impl Clock {
    /// Du départ, à `start`, à l'arrivée, à `finish`, au bout de `meters`, à allure constante.
    pub fn between(start: f64, finish: f64, meters: f64) -> Clock {
        Clock { marks: vec![[0.0, start], [meters.max(0.0), finish]] }
    }

    /// Les heures de passage d'une activité enregistrée (`Tour::timeline`), au moins deux.
    pub fn along(marks: Vec<[f64; 2]>) -> Option<Clock> {
        (marks.len() >= 2).then_some(Clock { marks })
    }

    pub fn start(&self) -> f64 {
        self.marks[0][1]
    }

    pub fn finish(&self) -> f64 {
        self.marks[self.marks.len() - 1][1]
    }

    /// L'instant du passage à `meters`.
    pub fn at(&self, meters: f64) -> f64 {
        let next = self.marks.partition_point(|mark| mark[0] < meters);
        if next == 0 {
            return self.start();
        }
        if next == self.marks.len() {
            return self.finish();
        }
        let ([from, start], [to, end]) = (self.marks[next - 1], self.marks[next]);
        if to <= from {
            return end;
        }
        start + (end - start) * (meters - from) / (to - from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_follows_the_recorded_pace() {
        let steady = Clock::between(1000.0, 2000.0, 500.0);
        assert_eq!(steady.at(250.0), 1500.0);
        assert_eq!(steady.at(-10.0), 1000.0);
        assert_eq!(steady.at(900.0), 2000.0);
        // Le premier kilomètre en 10 minutes, le second en 20.
        let recorded = Clock::along(vec![[0.0, 0.0], [1000.0, 600.0], [2000.0, 1800.0]]).unwrap();
        assert_eq!(recorded.at(500.0), 300.0);
        assert_eq!(recorded.at(1500.0), 1200.0);
        assert!(Clock::along(vec![[0.0, 0.0]]).is_none());
    }
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
