//! Altitudes des tuiles de relief Terrarium, décodées une fois : l'ombrage du fond (`hillshade`) et le
//! relief de la vue inclinée (`mesh`, la caméra, le tracé posé au sol) lisent les mêmes. Une source peut
//! en avoir une autre qui la comble (l'IGN, Terrarium) : hors de ses zones et dans ses trous.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use skia_safe::{AlphaType, ColorType, Data, Image, ImageInfo};

use crate::cache::Lru;
use crate::geo::LonLat;
use crate::mercator::{project, unproject, world_size};
use crate::model::TileSource;
use crate::tiles::TileKey;

/// Côté d'une tuile Terrarium, en pixels.
pub const DIM: usize = 256;
/// Tuiles décodées gardées : 256 Ko chacune.
const DECODED_TILES: usize = 512;
/// Sous cette altitude, un pixel Terrarium est un trou de la source (−32768 m), pas de la bathymétrie
/// (−11 000 m au plus profond) : des tuiles entières du zoom 14 en sont faites.
const VOID_BELOW: f32 = -12_000.0;

pub struct Elevation {
    pub name: String,
    pub maxzoom: u8,
    /// En deçà, la source qui comble sert seule.
    minzoom: u8,
    dir: PathBuf,
    /// Zones couvertes, `[ouest, sud, est, nord]` en degrés ; vide : le monde entier.
    bounds: Vec<[f64; 4]>,
    /// Ce qui comble cette source hors de ses zones, là où une tuile manque, et dans ses trous.
    fallback: Option<Arc<Elevation>>,
    tiles: Lru<TileKey, Option<Arc<Vec<f32>>>>,
}

impl Elevation {
    pub fn new(cache_dir: &str, name: &str, maxzoom: u8) -> Self {
        Elevation {
            name: name.to_owned(),
            maxzoom,
            minzoom: 0,
            dir: PathBuf::from(cache_dir).join(name),
            bounds: Vec::new(),
            fallback: None,
            tiles: Lru::new(DECODED_TILES),
        }
    }

    /// La source, et celle qui la comble, de proche en proche.
    pub fn of(cache_dir: &str, source: &TileSource) -> Self {
        Elevation {
            minzoom: source.minzoom,
            bounds: source.bounds.clone(),
            fallback: source.fallback.as_ref().map(|fallback| Arc::new(Elevation::of(cache_dir, fallback))),
            ..Elevation::new(cache_dir, &source.name, source.maxzoom)
        }
    }

    /// La tuile touche une zone de la source.
    fn covers(&self, key: TileKey) -> bool {
        if key.z < self.minzoom {
            return false;
        }
        if self.bounds.is_empty() {
            return true;
        }
        let corner = |x: u32, y: u32| unproject([f64::from(x) * 512.0, f64::from(y) * 512.0], f64::from(key.z));
        let ([west, north], [east, south]) = (corner(key.x, key.y), corner(key.x + 1, key.y + 1));
        self.bounds.iter().any(|&[w, s, e, n]| west < e && east > w && south < n && north > s)
    }

    /// Altitudes de la tuile en mètres, ligne par ligne ; `None` si elle manque au cache et qu'aucune source
    /// ne la comble. Ses trous prennent l'altitude de la source qui comble, sinon de la tuile parente
    /// (`tile_names`), à défaut 0.
    pub fn tile(&self, key: TileKey) -> Option<Arc<Vec<f32>>> {
        self.tiles.get_or_insert_with(&key, || {
            let file = self.dir.join(key.z.to_string()).join(key.x.to_string()).join(key.y.to_string());
            let bytes = self.covers(key).then(|| std::fs::read(file).ok()).flatten();
            let Some(bytes) = bytes else {
                // Hors des zones, ou tuile absente : toute de la source qui comble.
                self.fallback.as_ref()?;
                let mut voids = vec![f32::MIN; DIM * DIM];
                self.fill_voids(key, &mut voids);
                return Some(Arc::new(voids));
            };
            let image = Image::from_encoded(Data::new_copy(&bytes))?;
            let info = ImageInfo::new((DIM as i32, DIM as i32), ColorType::RGBA8888, AlphaType::Unpremul, None);
            let mut pixels = vec![0u8; DIM * DIM * 4];
            if image.width() as usize != DIM
                || !image.read_pixels(&info, &mut pixels, DIM * 4, (0, 0), skia_safe::image::CachingHint::Disallow)
            {
                return None;
            }
            // Terrarium : (R × 256 + G + B / 256) − 32768 mètres.
            let mut decoded: Vec<f32> = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| f32::from(p[0]) * 256.0 + f32::from(p[1]) + f32::from(p[2]) / 256.0 - 32768.0)
                .collect();
            if decoded.iter().any(|&altitude| altitude < VOID_BELOW) {
                self.fill_voids(key, &mut decoded);
            }
            Some(Arc::new(decoded))
        })
    }

    /// Bouche les trous de la tuile `key` par la source qui comble, au zoom le plus proche qu'elle ait,
    /// sinon par la tuile parente, bilinéaire, elle-même bouchée de proche en proche ; sans parente en
    /// cache, au niveau 0.
    fn fill_voids(&self, key: TileKey, altitudes: &mut [f32]) {
        if let Some(fallback) = &self.fallback {
            let z = key.z.min(fallback.maxzoom);
            let scale = f64::from(1u32 << (key.z - z));
            let (left, top) = (f64::from(key.x) * DIM as f64, f64::from(key.y) * DIM as f64);
            for (i, altitude) in altitudes.iter_mut().enumerate().filter(|(_, altitude)| **altitude < VOID_BELOW) {
                let (x, y) = ((i % DIM) as f64, (i / DIM) as f64);
                // Centre du pixel (x + 0,5) au zoom de la source qui comble, ramené aux centres entiers.
                let (px, py) = ((left + x + 0.5) / scale - 0.5, (top + y + 0.5) / scale - 0.5);
                *altitude = fallback.altitude_at_pixel(z, px, py) as f32;
            }
            return;
        }
        let parent = (key.z > 0).then(|| TileKey { z: key.z - 1, x: key.x / 2, y: key.y / 2 });
        let (left, top) = (f64::from(key.x % 2) * DIM as f64 / 2.0, f64::from(key.y % 2) * DIM as f64 / 2.0);
        for (i, altitude) in altitudes.iter_mut().enumerate().filter(|(_, altitude)| **altitude < VOID_BELOW) {
            let (x, y) = ((i % DIM) as f64, (i / DIM) as f64);
            *altitude = match parent {
                // Centre du pixel (x + 0,5) au zoom z − 1, ramené aux centres entiers.
                Some(parent) => {
                    let (px, py) = (left + (x + 0.5) / 2.0 - 0.5, top + (y + 0.5) / 2.0 - 0.5);
                    self.altitude_at_pixel(
                        parent.z,
                        f64::from(parent.x) * DIM as f64 + px,
                        f64::from(parent.y) * DIM as f64 + py,
                    ) as f32
                }
                None => 0.0,
            };
        }
    }

    /// Les tuiles parentes de `tiles`, qui bouchent leurs trous.
    pub fn parent_tiles(tiles: &[TileKey]) -> Vec<TileKey> {
        tiles.iter().filter(|t| t.z > 0).map(|t| TileKey { z: t.z - 1, x: t.x / 2, y: t.y / 2 }).collect()
    }

    /// Les fichiers à lire pour les tuiles `tiles`, `<source>/<z>/<x>/<y>` : celles des zones de la source,
    /// et ce qui comble leurs trous, la source qui comble (tuiles voisines comprises, que l'interpolation
    /// lit au bord) ou à défaut les tuiles parentes.
    pub fn tile_names(&self, tiles: &[TileKey]) -> BTreeSet<String> {
        let name = |source: &str, t: &TileKey| format!("{source}/{}/{}/{}", t.z, t.x, t.y);
        let mut names: BTreeSet<String> =
            tiles.iter().filter(|t| self.covers(**t)).map(|t| name(&self.name, t)).collect();
        match &self.fallback {
            Some(fallback) => {
                let mut below = BTreeSet::new();
                for t in tiles {
                    let z = t.z.min(fallback.maxzoom);
                    let (x, y) = (i64::from(t.x >> (t.z - z)), i64::from(t.y >> (t.z - z)));
                    let count = 1i64 << z;
                    for (dx, dy) in [-1, 0, 1].into_iter().flat_map(|dx| [-1, 0, 1].map(|dy| (dx, dy))) {
                        let y = y + dy;
                        if (0..count).contains(&y) {
                            below.insert(TileKey { z, x: (x + dx).rem_euclid(count) as u32, y: y as u32 });
                        }
                    }
                }
                names.extend(fallback.tile_names(&below.into_iter().collect::<Vec<_>>()));
            }
            None => names.extend(Self::parent_tiles(tiles).iter().map(|t| name(&self.name, t))),
        }
        names
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

    /// Tuile Terrarium uniforme à `altitude` (trou de la source sous −32 000 m), dans `dir`.
    fn write_tile(dir: &std::path::Path, key: TileKey, altitude: f64) {
        let value = (altitude + 32768.0).max(0.0);
        let pixel = [(value / 256.0).floor() as u8, (value % 256.0).floor() as u8, 0, 255];
        let pixels: Vec<u8> = pixel.repeat(DIM * DIM);
        let info = ImageInfo::new((DIM as i32, DIM as i32), ColorType::RGBA8888, AlphaType::Unpremul, None);
        let image = skia_safe::images::raster_from_data(&info, Data::new_copy(&pixels), DIM * 4).unwrap();
        let png = image.encode(None, skia_safe::EncodedImageFormat::PNG, 100).unwrap();
        let folder = dir.join(key.z.to_string()).join(key.x.to_string());
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(key.y.to_string()), png.as_bytes()).unwrap();
    }

    #[test]
    fn voids_take_the_altitude_of_the_parent_tile() {
        let dir = std::env::temp_dir().join(format!("dijkstravel-voids-{}", std::process::id()));
        let root = dir.join("terrarium");
        write_tile(&root, TileKey { z: 13, x: 4243, y: 2899 }, -32768.0);
        write_tile(&root, TileKey { z: 12, x: 2121, y: 1449 }, 500.0);
        write_tile(&root, TileKey { z: 13, x: 4300, y: 2899 }, -32768.0);
        let elevation = Elevation::new(dir.to_str().unwrap(), "terrarium", 14);
        let filled = elevation.tile(TileKey { z: 13, x: 4243, y: 2899 }).unwrap();
        // La dernière ligne et la dernière colonne mêlent la parente voisine, absente.
        let inner = filled.iter().enumerate().filter(|(i, _)| i % DIM < DIM - 1 && i / DIM < DIM - 1);
        assert!(inner.into_iter().all(|(_, &altitude)| (altitude - 500.0).abs() < 1.0));
        // Sans parente en cache, au niveau de la mer comme une tuile absente.
        let orphan = elevation.tile(TileKey { z: 13, x: 4300, y: 2899 }).unwrap();
        assert!(orphan.iter().all(|&altitude| altitude == 0.0));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// L'IGN au zoom 14 en métropole, comblé par Terrarium au zoom 13.
    fn ign_over_terrarium(dir: &std::path::Path) -> Elevation {
        let terrarium =
            TileSource { name: "terrarium".into(), maxzoom: 13, minzoom: 0, bounds: Vec::new(), fallback: None };
        let ign = TileSource {
            name: "ign-alti".into(),
            maxzoom: 14,
            minzoom: 7,
            bounds: vec![[-5.5, 41.2, 9.8, 51.2]],
            fallback: Some(Box::new(terrarium)),
        };
        Elevation::of(dir.to_str().unwrap(), &ign)
    }

    #[test]
    fn the_ign_is_read_in_france_and_terrarium_fills_the_rest() {
        let dir = std::env::temp_dir().join(format!("dijkstravel-ign-{}", std::process::id()));
        // Belledonne (6,0° E, 45,2° N) dans le cadre de la métropole, le Portugal (9,0° O) hors de lui.
        let alps = TileKey { z: 14, x: 8465, y: 5873 };
        write_tile(&dir.join("ign-alti"), alps, -32768.0);
        write_tile(&dir.join("terrarium"), TileKey { z: 13, x: 4232, y: 2936 }, 1800.0);
        let elevation = ign_over_terrarium(&dir);
        let names = elevation.tile_names(&[alps]);
        assert!(names.contains("ign-alti/14/8465/5873"));
        assert!(names.contains("terrarium/13/4232/2936") && names.contains("terrarium/13/4233/2937"), "{names:?}");
        let portugal = TileKey { z: 14, x: 7783, y: 6193 };
        assert!(!elevation.tile_names(&[portugal]).iter().any(|name| name.starts_with("ign-alti")));
        // Vue de loin, au-dessus de la France : Terrarium seul.
        let far = TileKey { z: 5, x: 16, y: 11 };
        assert!(!elevation.tile_names(&[far]).iter().any(|name| name.starts_with("ign-alti")));
        // Trou de l'IGN : l'altitude de Terrarium.
        let filled = elevation.tile(alps).unwrap();
        assert!(filled[DIM * 10 + 10] > 1799.0 && filled[DIM * 10 + 10] < 1801.0, "{}", filled[DIM * 10 + 10]);
        // Hors zone, sans fichier de l'IGN : toute de Terrarium, au niveau 0 sans tuile en cache.
        assert!(elevation.tile(portugal).unwrap().iter().all(|&altitude| altitude == 0.0));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_tiles_are_at_sea_level() {
        let elevation = Elevation::new("/nonexistent", "terrarium", 14);
        assert_eq!(elevation.altitude_at([6.86, 45.83], 12), 0.0);
    }
}
