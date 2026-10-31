//! Altitudes des tuiles de relief Terrarium, décodées une fois : l'ombrage du fond (`hillshade`) et le
//! relief de la vue inclinée (`mesh`, la caméra, le tracé posé au sol) lisent les mêmes.

use std::path::PathBuf;
use std::sync::Arc;

use skia_safe::{AlphaType, ColorType, Data, Image, ImageInfo};

use crate::cache::Lru;
use crate::geo::LonLat;
use crate::mercator::{project, world_size};
use crate::tiles::TileKey;

/// Côté d'une tuile Terrarium, en pixels.
pub const DIM: usize = 256;
/// Tuiles décodées gardées : 256 Ko chacune.
const DECODED_TILES: usize = 512;

pub struct Elevation {
    pub name: String,
    pub maxzoom: u8,
    dir: PathBuf,
    tiles: Lru<TileKey, Option<Arc<Vec<f32>>>>,
}

impl Elevation {
    pub fn new(cache_dir: &str, name: &str, maxzoom: u8) -> Self {
        Elevation {
            name: name.to_owned(),
            maxzoom,
            dir: PathBuf::from(cache_dir).join(name),
            tiles: Lru::new(DECODED_TILES),
        }
    }

    /// Altitudes de la tuile en mètres, ligne par ligne ; `None` si elle manque au cache.
    pub fn tile(&self, key: TileKey) -> Option<Arc<Vec<f32>>> {
        self.tiles.get_or_insert_with(&key, || {
            let file = self.dir.join(key.z.to_string()).join(key.x.to_string()).join(key.y.to_string());
            let image = Image::from_encoded(Data::new_copy(&std::fs::read(file).ok()?))?;
            let info = ImageInfo::new((DIM as i32, DIM as i32), ColorType::RGBA8888, AlphaType::Unpremul, None);
            let mut pixels = vec![0u8; DIM * DIM * 4];
            if image.width() as usize != DIM
                || !image.read_pixels(&info, &mut pixels, DIM * 4, (0, 0), skia_safe::image::CachingHint::Disallow)
            {
                return None;
            }
            // Terrarium : (R × 256 + G + B / 256) − 32768 mètres.
            let decoded = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| f32::from(p[0]) * 256.0 + f32::from(p[1]) + f32::from(p[2]) / 256.0 - 32768.0)
                .collect();
            Some(Arc::new(decoded))
        })
    }

    /// Tuiles au zoom `z` qui couvrent `[x0, x1[ × [y0, y1[` (pixels monde au zoom `level_zoom`), plus
    /// `apron` tuiles autour.
    pub fn tiles_covering(&self, z: u8, level_zoom: f64, [x0, y0, x1, y1]: [f64; 4], apron: i64) -> Vec<TileKey> {
        let tile = world_size(level_zoom) / f64::from(1u32 << z);
        let count = 1i64 << z;
        let mut tiles = Vec::new();
        let ys = ((y0 / tile).floor() as i64 - apron).max(0)..=((y1 / tile).ceil() as i64 - 1 + apron).min(count - 1);
        for y in ys {
            for x in (x0 / tile).floor() as i64 - apron..=(x1 / tile).ceil() as i64 - 1 + apron {
                tiles.push(TileKey { z, x: x.rem_euclid(count) as u32, y: y as u32 });
            }
        }
        tiles
    }

    /// Tuiles au zoom `z` que lit `altitude_at` pour ce point (les quatre pixels qui l'entourent).
    pub fn tiles_at(&self, point: LonLat, z: u8) -> Vec<TileKey> {
        let [x, y] = project(point, f64::from(z));
        let scale = DIM as f64 / 512.0;
        let (px, py) = (x * scale - 0.5, y * scale - 0.5);
        let (size, dim) = ((DIM as i64) << z, DIM as i64);
        let mut tiles: Vec<TileKey> = [(0, 0), (1, 0), (0, 1), (1, 1)]
            .iter()
            .map(|&(dx, dy)| {
                let (x, y) = ((px.floor() as i64 + dx).rem_euclid(size), (py.floor() as i64 + dy).clamp(0, size - 1));
                TileKey { z, x: (x / dim) as u32, y: (y / dim) as u32 }
            })
            .collect();
        tiles.sort();
        tiles.dedup();
        tiles
    }

    /// Altitude du sol en mètres, bilinéaire entre les pixels des tuiles au zoom `z`. La mer et ce qui
    /// manque au cache sont au niveau 0 : le relief ne descend pas sous la mer (bathymétrie de Terrarium).
    pub fn altitude_at(&self, point: LonLat, z: u8) -> f64 {
        let [x, y] = project(point, f64::from(z));
        // Pixels de tuile au zoom z (tuiles de 256, monde de 512 × 2^z) ; centres en k + 0,5.
        let scale = DIM as f64 / 512.0;
        self.altitude_at_pixel(z, x * scale - 0.5, y * scale - 0.5)
    }

    /// Altitude au pixel de relief `(px, py)` du zoom `z` (centres des pixels aux entiers).
    pub fn altitude_at_pixel(&self, z: u8, px: f64, py: f64) -> f64 {
        let size = (DIM as i64) << z;
        let (x0, y0) = (px.floor(), py.floor());
        let (tx, ty) = (px - x0, py - y0);
        let mut current: Option<(TileKey, Option<Arc<Vec<f32>>>)> = None;
        let mut sample = |x: i64, y: i64| -> f64 {
            let (x, y) = (x.rem_euclid(size), y.clamp(0, size - 1));
            let key = TileKey { z, x: (x / DIM as i64) as u32, y: (y / DIM as i64) as u32 };
            if current.as_ref().is_none_or(|(k, _)| *k != key) {
                current = Some((key, self.tile(key)));
            }
            match current.as_ref().and_then(|(_, tile)| tile.as_ref()) {
                Some(tile) => f64::from(tile[(y as usize % DIM) * DIM + x as usize % DIM]).max(0.0),
                None => 0.0,
            }
        };
        let (x0, y0) = (x0 as i64, y0 as i64);
        let (a, b, c, d) = (sample(x0, y0), sample(x0 + 1, y0), sample(x0, y0 + 1), sample(x0 + 1, y0 + 1));
        let top = a + (b - a) * tx;
        let bottom = c + (d - c) * tx;
        top + (bottom - top) * ty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_the_tiles_under_a_box_and_its_apron() {
        let elevation = Elevation::new("/nonexistent", "terrarium", 14);
        // Au zoom 2, une tuile de relief couvre 1024 pixels monde au zoom 3 (monde de 4096).
        let tiles = elevation.tiles_covering(2, 3.0, [1100.0, 1100.0, 1900.0, 2100.0], 0);
        assert_eq!(tiles, vec![TileKey { z: 2, x: 1, y: 1 }, TileKey { z: 2, x: 1, y: 2 }]);
        // Bordure d'une tuile ; au bord ouest du monde, elle reprend à l'est.
        assert_eq!(elevation.tiles_covering(2, 3.0, [1100.0, 1100.0, 1900.0, 2000.0], 1).len(), 9);
        let west = elevation.tiles_covering(2, 3.0, [10.0, 1100.0, 20.0, 1200.0], 1);
        assert!(west.contains(&TileKey { z: 2, x: 3, y: 1 }));
    }

    #[test]
    fn missing_tiles_are_at_sea_level() {
        let elevation = Elevation::new("/nonexistent", "terrarium", 14);
        assert_eq!(elevation.altitude_at([6.86, 45.83], 12), 0.0);
    }
}
