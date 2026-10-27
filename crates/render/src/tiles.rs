//! Tuiles raster lues dans le cache disque que remplit `download` avant le rendu : le moteur ne
//! télécharge rien, une tuile absente (hors couverture) est simplement transparente.

use std::path::PathBuf;

use skia_safe::{Data, Image};

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
            Some(image.with_default_mipmaps().unwrap_or(image))
        })
    }

    pub fn clear(&self) {
        self.decoded.clear();
    }
}
