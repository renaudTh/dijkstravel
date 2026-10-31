//! Relief de la vue inclinée : un maillage par morceau d'atlas, une grille d'altitudes que le GPU
//! texture avec ce morceau. Les morceaux lointains viennent des niveaux d'atlas plus grossiers ; une
//! « jupe » qui descend de chaque bord cache les fentes entre deux niveaux voisins.

use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

use crate::atlas::{CHUNK, ChunkKey};
use crate::cache::Lru;
use crate::elevation::{DIM, Elevation};
use crate::mercator::{unproject, world_size};

/// Cellules de la grille sur le côté d'un morceau : 8 pixels monde chacune.
pub const GRID: usize = 64;
/// Maillages gardés : 50 Ko chacun.
const MESHES: usize = 512;
/// Profondeur de la jupe, en part du côté du morceau au sol.
const SKIRT_SHARE: f64 = 1.0 / 16.0;
/// Le relief est lu à ce nombre de zooms sous le niveau du morceau : un pixel de relief par cellule.
const ZOOMS_BELOW_LEVEL: f64 = 2.0;
const EARTH_CIRCUMFERENCE: f64 = 40_075_016.686;

/// Sommet : position dans le morceau (0 à 1, aussi coordonnée de texture) et altitude en mètres.
pub type Vertex = [f32; 3];

pub struct Meshes {
    elevation: Arc<Elevation>,
    travel_zoom: f64,
    meshes: Lru<ChunkKey, Arc<Vec<Vertex>>>,
}

impl Meshes {
    /// `travel_zoom` : celui du niveau 0 de l'atlas.
    pub fn new(elevation: Arc<Elevation>, travel_zoom: f64) -> Self {
        Meshes { elevation, travel_zoom, meshes: Lru::new(MESHES) }
    }

    fn level_zoom(&self, level: u32) -> f64 {
        self.travel_zoom - f64::from(level)
    }

    /// Zoom des tuiles de relief d'un morceau.
    pub fn tile_zoom(&self, level: u32) -> u8 {
        (self.level_zoom(level) - ZOOMS_BELOW_LEVEL).round().clamp(0.0, f64::from(self.elevation.maxzoom)) as u8
    }

    /// Sommets du morceau : la grille, ligne par ligne, puis la jupe, dans l'ordre de `perimeter`.
    pub fn mesh(&self, key: ChunkKey) -> Arc<Vec<Vertex>> {
        self.meshes.get_or_insert_with(&key, || {
            let level_zoom = self.level_zoom(key.level);
            let z = self.tile_zoom(key.level);
            // Pixels de relief (zoom z) par pixel monde du niveau.
            let to_dem = (DIM as f64 * f64::from(1u32 << z)) / world_size(level_zoom);
            let chunk = f64::from(CHUNK);
            let cell = chunk / GRID as f64;
            let (x0, y0) = (key.x as f64 * chunk, key.y as f64 * chunk);
            let mut vertices: Vec<Vertex> = Vec::with_capacity((GRID + 1) * (GRID + 1) + 4 * GRID);
            for j in 0..=GRID {
                for i in 0..=GRID {
                    let (x, y) = (x0 + i as f64 * cell, y0 + j as f64 * cell);
                    let altitude = self.elevation.altitude_at_pixel(z, x * to_dem - 0.5, y * to_dem - 0.5);
                    vertices.push([i as f32 / GRID as f32, j as f32 / GRID as f32, altitude as f32]);
                }
            }
            let latitude = unproject([x0 + chunk / 2.0, y0 + chunk / 2.0], level_zoom)[1];
            let chunk_meters = chunk * EARTH_CIRCUMFERENCE * latitude.to_radians().cos() / world_size(level_zoom);
            let depth = (chunk_meters * SKIRT_SHARE) as f32;
            for &index in perimeter() {
                let [u, v, altitude] = vertices[index as usize];
                vertices.push([u, v, altitude - depth]);
            }
            Arc::new(vertices)
        })
    }

    /// Tuiles de relief que lit le maillage d'un morceau, `<source>/<z>/<x>/<y>`.
    pub fn tiles_of(&self, key: ChunkKey) -> BTreeSet<String> {
        let level_zoom = self.level_zoom(key.level);
        let chunk = f64::from(CHUNK);
        let (x0, y0) = (key.x as f64 * chunk, key.y as f64 * chunk);
        // Le bord droit et le bas du morceau lisent le premier pixel de la tuile voisine.
        let margin = world_size(level_zoom) / ((DIM as f64) * f64::from(1u32 << self.tile_zoom(key.level)));
        let bounds = [x0, y0, x0 + chunk + margin, y0 + chunk + margin];
        let name = &self.elevation.name;
        self.elevation
            .tiles_covering(self.tile_zoom(key.level), level_zoom, bounds, 0)
            .into_iter()
            .map(|t| format!("{name}/{}/{}/{}", t.z, t.x, t.y))
            .collect()
    }
}

/// Indices des sommets du bord de la grille, en un tour : haut, droite, bas, gauche.
fn perimeter() -> &'static [u32] {
    static PERIMETER: OnceLock<Vec<u32>> = OnceLock::new();
    PERIMETER.get_or_init(|| {
        let at = |i: usize, j: usize| (j * (GRID + 1) + i) as u32;
        let top = (0..GRID).map(|i| at(i, 0));
        let right = (0..GRID).map(|j| at(GRID, j));
        let bottom = (0..GRID).map(|i| at(GRID - i, GRID));
        let left = (0..GRID).map(|j| at(0, GRID - j));
        top.chain(right).chain(bottom).chain(left).collect()
    })
}

/// Triangles de tous les maillages : la grille, puis la jupe.
pub fn indices() -> &'static [u32] {
    static INDICES: OnceLock<Vec<u32>> = OnceLock::new();
    INDICES.get_or_init(|| {
        let at = |i: usize, j: usize| (j * (GRID + 1) + i) as u32;
        let mut indices = Vec::with_capacity(GRID * GRID * 6 + 4 * GRID * 6);
        for j in 0..GRID {
            for i in 0..GRID {
                let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1));
                indices.extend([a, c, b, b, c, d]);
            }
        }
        let perimeter = perimeter();
        let skirt = ((GRID + 1) * (GRID + 1)) as u32;
        let count = perimeter.len();
        for k in 0..count {
            let next = (k + 1) % count;
            let (a, b) = (perimeter[k], perimeter[next]);
            let (low_a, low_b) = (skirt + k as u32, skirt + next as u32);
            indices.extend([a, low_a, b, b, low_a, low_b]);
        }
        indices
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skirt_hangs_under_every_border_vertex() {
        let meshes = Meshes::new(Arc::new(Elevation::new("/nonexistent", "terrarium", 14)), 13.0);
        let key = ChunkKey { level: 0, x: 4242, y: 2929 };
        let mesh = meshes.mesh(key);
        let grid = (GRID + 1) * (GRID + 1);
        assert_eq!(mesh.len(), grid + 4 * GRID);
        // Sans tuile, le sol est à 0 et la jupe dessous.
        assert!(mesh[..grid].iter().all(|v| v[2] == 0.0));
        assert!(mesh[grid..].iter().all(|v| v[2] < 0.0));
        assert_eq!(mesh[grid][..2], mesh[0][..2]);
        let indices = indices();
        assert_eq!(indices.len() % 3, 0);
        assert!(indices.iter().all(|&i| (i as usize) < mesh.len()));
        // Relief lu deux zooms sous le niveau, au plus au zoom maximal de la source.
        assert_eq!(meshes.tile_zoom(0), 11);
        assert_eq!(Meshes::new(Arc::new(Elevation::new("/x", "terrarium", 14)), 17.6).tile_zoom(0), 14);
        // Un morceau de 512 pixels au zoom 13 couvre un quart de tuile de relief au zoom 11 (et sa voisine
        // pour le dernier pixel).
        assert!(!meshes.tiles_of(key).is_empty() && meshes.tiles_of(key).len() <= 4);
    }
}
