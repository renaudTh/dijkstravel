//! Tuiles raster lues dans le cache disque que remplit `download` avant le rendu : le moteur ne
//! télécharge rien, une tuile absente (hors couverture) est simplement transparente. L'IGN peint la mer
//! en blanc : dans une tuile surtout blanche, ce blanc devient transparent et le fond du thème le remplace.

use std::path::PathBuf;

use skia_safe::{AlphaType, ColorType, Data, Image, ImageInfo};

use crate::cache::Lru;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

/// Tuiles d'une source : `<dossier>/<nom>/<z>/<x>/<y>`, décodées une fois puis gardées en mémoire.
pub struct TileStore {
    pub name: String,
    pub maxzoom: u8,
    dir: PathBuf,
    decoded: Lru<TileKey, Option<Image>>,
}

/// Tuiles décodées gardées : 256 Ko chacune, environ 256 Mo.
const DECODED_TILES: usize = 1024;

impl TileStore {
    pub fn new(cache_dir: &str, name: &str, maxzoom: u8) -> Self {
        TileStore {
            name: name.to_owned(),
            maxzoom,
            dir: PathBuf::from(cache_dir).join(name),
            decoded: Lru::new(DECODED_TILES),
        }
    }

    pub fn get(&self, key: TileKey) -> Option<Image> {
        self.decoded.get_or_insert_with(&key, || {
            let file = self.dir.join(key.z.to_string()).join(key.x.to_string()).join(key.y.to_string());
            let bytes = std::fs::read(file).ok()?;
            // Décodée tout de suite (une image encodée serait redécodée à chaque dessin), mipmaps comprises.
            let image = Image::from_encoded(Data::new_copy(&bytes))?.make_raster_image(None, None)?;
            let image = without_sea(image)?;
            Some(image.with_default_mipmaps().unwrap_or(image))
        })
    }

    pub fn clear(&self) {
        self.decoded.clear();
    }
}

/// Pixel de mer de l'IGN : blanc, aux artefacts JPEG près.
const SEA_WHITE: u8 = 240;
/// Part de pixels blancs au-delà de laquelle une tuile montre surtout la mer, et pas de la neige ou un toit.
const SEA_SHARE: f64 = 0.5;

/// La tuile, ses pixels de mer rendus transparents si elle en est surtout faite ; rien si elle n'est
/// que mer.
fn without_sea(image: Image) -> Option<Image> {
    let (width, height) = (image.width(), image.height());
    let info = ImageInfo::new((width, height), ColorType::RGBA8888, AlphaType::Premul, None);
    let row_bytes = width as usize * 4;
    let mut pixels = vec![0u8; row_bytes * height as usize];
    if !image.read_pixels(&info, &mut pixels, row_bytes, (0, 0), skia_safe::image::CachingHint::Allow) {
        return Some(image);
    }
    let is_sea = |pixel: &[u8; 4]| pixel[..3].iter().all(|&byte| byte >= SEA_WHITE);
    let (all, sea) = (pixels.len() / 4, pixels.as_chunks::<4>().0.iter().filter(|pixel| is_sea(pixel)).count());
    if (sea as f64) < SEA_SHARE * all as f64 {
        return Some(image);
    }
    // Quelques pixels qui bavent ne font pas une côte.
    if all - sea <= all / 200 {
        return None;
    }
    for pixel in pixels.as_chunks_mut::<4>().0 {
        if is_sea(pixel) {
            *pixel = [0; 4];
        }
    }
    skia_safe::images::raster_from_data(&info, Data::new_copy(&pixels), row_bytes)
}

#[cfg(test)]
mod tests {
    use skia_safe::{Color, Paint, surfaces};

    use super::without_sea;

    #[test]
    fn sea_turns_transparent_but_land_stays() {
        let mut surface = surfaces::raster_n32_premul((16, 16)).unwrap();
        surface.canvas().clear(Color::WHITE);
        assert!(without_sea(surface.image_snapshot()).is_none());
        // Une côte : la moitié gauche est de la terre.
        let mut land = Paint::default();
        land.set_color(Color::from_rgb(60, 80, 40));
        surface.canvas().draw_rect(skia_safe::Rect::from_xywh(0.0, 0.0, 6.0, 16.0), &land);
        let coast = without_sea(surface.image_snapshot()).unwrap();
        let pixel = |image: &skia_safe::Image, x: i32| {
            let mut out = [0u8; 4];
            let info =
                skia_safe::ImageInfo::new((1, 1), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Premul, None);
            image.read_pixels(&info, &mut out, 4, (x, 8), skia_safe::image::CachingHint::Allow);
            out
        };
        assert_eq!(pixel(&coast, 2), [60, 80, 40, 255]);
        assert_eq!(pixel(&coast, 12), [0, 0, 0, 0]);
        // Une tuile de terre, même avec de la neige, ne change pas.
        surface.canvas().clear(Color::from_rgb(60, 80, 40));
        let mut snow = Paint::default();
        snow.set_color(Color::WHITE);
        surface.canvas().draw_rect(skia_safe::Rect::from_xywh(0.0, 0.0, 4.0, 16.0), &snow);
        assert_eq!(pixel(&without_sea(surface.image_snapshot()).unwrap(), 2), [255, 255, 255, 255]);
    }
}
