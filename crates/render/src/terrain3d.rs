//! Vue inclinée d'un rendu : quels morceaux d'atlas dessiner en relief et à quel niveau, leur rendu sur
//! le GPU, et la projection qui pose le tracé, les lieux et les repères sur ce relief en cachant ce qui
//! passe derrière.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

use glam::DVec2;

use crate::atlas::{APRON, Atlas, CHUNK, ChunkKey};
use crate::color::parse_color;
use crate::elevation::Elevation;
use crate::frame::Camera;
use crate::geo::LonLat;
use crate::gpu::{ChunkDraw, FrameSettings, Gpu, Rendered};
use crate::mercator::Projection;
use crate::mesh::Meshes;
use crate::model::BasemapTheme;
use crate::perspective::Perspective;

/// Niveaux d'atlas au plus au-dessus de celui de la caméra, pour les morceaux lointains.
const EXTRA_LEVELS: u32 = 6;
/// Ciel et brume, avant le voile du thème.
const SKY: &str = "#7fa9d6";
const HAZE: &str = "#dfe8f0";
/// Un point est caché si le relief devant lui est plus proche de cette part de sa distance.
const OCCLUSION_MARGIN: f64 = 0.01;

/// GPU ouverts, un par taille d'image.
type GpusBySize = HashMap<(u32, u32), Arc<Gpu>>;

pub struct Terrain3d {
    gpu: Arc<Gpu>,
    elevation: Arc<Elevation>,
    meshes: Meshes,
    exaggeration: f64,
    travel_zoom: f64,
    sky: [f32; 4],
    haze: [f32; 4],
}

impl Terrain3d {
    /// Le GPU est ouvert une fois par taille d'image et partagé par tous les rendus du processus.
    pub fn new(
        atlas: &Atlas,
        basemap: &BasemapTheme,
        exaggeration: f64,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        static GPUS: OnceLock<Mutex<GpusBySize>> = OnceLock::new();
        let gpu = {
            let mut gpus = GPUS.get_or_init(Default::default).lock().unwrap();
            match gpus.get(&(width, height)) {
                Some(gpu) => gpu.clone(),
                None => {
                    let gpu = Arc::new(Gpu::new(width, height)?);
                    gpus.insert((width, height), gpu.clone());
                    gpu
                }
            }
        };
        // L'atmosphère prend le voile du thème, comme le fond.
        let tint = parse_color(&basemap.tint_color);
        let opacity = basemap.tint_opacity.clamp(0.0, 1.0) as f32;
        let veiled = |hex: &str| {
            let color = parse_color(hex);
            let mix = |a: u8, b: u8| (f32::from(a) * (1.0 - opacity) + f32::from(b) * opacity) / 255.0;
            [mix(color.r(), tint.r()), mix(color.g(), tint.g()), mix(color.b(), tint.b()), 1.0]
        };
        let elevation = atlas.elevation();
        Ok(Terrain3d {
            gpu,
            meshes: Meshes::new(elevation.clone(), atlas.travel_zoom()),
            elevation,
            exaggeration,
            travel_zoom: atlas.travel_zoom(),
            sky: veiled(SKY),
            haze: veiled(HAZE),
        })
    }

    /// Zoom du relief lu sous la caméra : celui des maillages de son niveau.
    fn tile_zoom(&self, atlas: &Atlas, camera: Camera) -> u8 {
        self.meshes.tile_zoom(atlas.level_for(camera.zoom))
    }

    /// La caméra au-dessus du relief moyenné autour du point visé (`sight::ground`).
    pub fn perspective(&self, camera: Camera, width: u32, height: u32) -> Perspective {
        let ground = crate::sight::ground(&self.elevation, camera, [width, height]);
        Perspective::new(camera, width, height, ground, self.exaggeration)
    }

    /// Morceaux à dessiner : chacun au niveau le plus grossier dont les pixels ne grossissent pas à
    /// l'écran, d'autant plus grossier qu'il est loin ; les niveaux s'emboîtent sans se chevaucher.
    pub fn chunks(&self, atlas: &Atlas, perspective: &Perspective) -> Vec<ChunkKey> {
        select_chunks(perspective, self.travel_zoom, atlas.level_for(perspective.camera().zoom))
    }

    /// Tuiles que lisent les morceaux (imagerie, ombrage, frontières) et leurs maillages.
    pub fn tiles_of(&self, atlas: &Atlas, chunks: &[ChunkKey]) -> BTreeSet<String> {
        chunks.iter().flat_map(|&key| atlas.tiles_of(key).into_iter().chain(self.meshes.tiles_of(key))).collect()
    }

    /// Ciel et relief de l'image, dessinés dans `canvas` ; `transfer` : le fond des transferts et sa part.
    pub fn render(
        &self,
        atlas: &Atlas,
        transfer: Option<(&Atlas, f64)>,
        perspective: &Perspective,
        canvas: &skia_safe::Canvas,
    ) -> Rendered {
        let chunks = self.chunks(atlas, perspective);
        let transfer = transfer.filter(|(_, blend)| *blend > 0.0);
        let side = f64::from(CHUNK + 2 * APRON);
        let eye = perspective.eye();
        let view_projection = perspective.view_projection().to_cols_array_2d().map(|c| c.map(|v| v as f32));
        let settings = FrameSettings {
            view_projection,
            eye: [eye.x as f32, eye.y as f32, eye.z as f32],
            z_per_meter: perspective.z_per_meter() as f32,
            ground: perspective.ground() as f32,
            fog: perspective.fog().map(|distance| distance as f32),
            fog_color: self.haze,
            sky_color: self.sky,
            horizon: perspective.horizon() as f32,
            transfer_blend: transfer.map_or(0.0, |(_, blend)| blend as f32),
            uv_inset: (f64::from(APRON) / side) as f32,
            uv_scale: (f64::from(CHUNK) / side) as f32,
        };
        let meshes: Vec<_> = chunks.iter().map(|&key| move || self.meshes.mesh(key)).collect();
        let images: Vec<_> = chunks.iter().map(|&key| move || atlas.layers(key)).collect();
        let transfer_images: Vec<_> =
            chunks.iter().map(|&key| move || transfer.map(|(transfer, _)| transfer.layers(key))).collect();
        let draws: Vec<ChunkDraw> = chunks
            .iter()
            .enumerate()
            .map(|(i, &key)| {
                let rect = local_rect(perspective, self.travel_zoom, key);
                ChunkDraw {
                    key: hash((atlas.id, key)),
                    mesh_key: hash((self.travel_zoom.to_bits(), key)),
                    transfer_key: transfer.map(|(transfer, _)| hash((transfer.id, key))),
                    origin: [rect.left as f32, rect.top as f32],
                    size: rect.size as f32,
                    mesh: &meshes[i],
                    image: &images[i],
                    transfer_image: &transfer_images[i],
                }
            })
            .collect();
        self.gpu.render(&settings, &draws, canvas)
    }

    /// La projection du tracé et des lieux sur le relief rendu ; `lift` : demi-largeur du tracé, en pixels de
    /// l'image, dont il peut dépasser d'une crête pour se voir.
    pub fn projection<'a>(
        &'a self,
        atlas: &Atlas,
        perspective: &'a Perspective,
        depth: &'a [f32],
        depth_size: [u32; 2],
        lift: f64,
    ) -> OnRelief<'a> {
        let chunks: HashSet<ChunkKey> = self.chunks(atlas, perspective).into_iter().collect();
        let mut levels: Vec<u32> = chunks.iter().map(|key| key.level).collect();
        levels.sort_unstable();
        levels.dedup();
        OnRelief {
            terrain: self,
            perspective,
            depth,
            zoom: self.tile_zoom(atlas, perspective.camera()),
            chunks,
            levels,
            lift,
            depth_size,
        }
    }
}

/// Points posés sur le relief ; ceux qu'il cache ne se voient pas.
pub struct OnRelief<'a> {
    terrain: &'a Terrain3d,
    perspective: &'a Perspective,
    depth: &'a [f32],
    zoom: u8,
    /// Les morceaux dessinés, et leurs niveaux : un point se pose sur celui qui le contient.
    chunks: HashSet<ChunkKey>,
    levels: Vec<u32>,
    /// Pixels de l'image dont un point peut être caché et rester vu : la demi-largeur du tracé.
    lift: f64,
    /// Taille de `depth`, plus petite que l'image pour une grande image.
    depth_size: [u32; 2],
}

impl OnRelief<'_> {
    /// Altitude du point sur le relief dessiné ; hors des morceaux, sur le relief sous la caméra.
    fn altitude(&self, point: LonLat) -> f64 {
        let meshes = &self.terrain.meshes;
        self.levels
            .iter()
            .map(|&level| meshes.key_at(point, level))
            .find(|key| self.chunks.contains(key))
            .map_or_else(|| self.terrain.elevation.altitude_at(point, self.zoom), |key| meshes.altitude_on(key, point))
    }

    /// Le relief cache le pixel `[x, y]` (dans l'image) à la profondeur `depth` ; hors de l'image, rien.
    fn hidden(&self, [x, y]: [f64; 2], depth: f64) -> bool {
        let [width, height] = self.perspective.size();
        if !(x >= 0.0 && y >= 0.0 && x < width && y < height) {
            return false;
        }
        let [depth_width, depth_height] = self.depth_size;
        let (dx, dy) = ((x * f64::from(depth_width) / width) as usize, (y * f64::from(depth_height) / height) as usize);
        let relief = f64::from(self.depth[dy.min(depth_height as usize - 1) * depth_width as usize + dx]);
        let (relief, own) = (self.perspective.linear_depth(relief), self.perspective.linear_depth(depth));
        relief < own * (1.0 - OCCLUSION_MARGIN)
    }
}

impl Projection for OnRelief<'_> {
    fn screen_point(&self, point: LonLat) -> Option<[f64; 2]> {
        let altitude = self.altitude(point);
        let ([x, y], depth) = self.perspective.screen_at(point, altitude)?;
        // Hors de l'écran, rien à cacher : un trait qui en sort reste entier.
        if self.hidden([x, y], depth) {
            // Un tracé qui longe une crête la frôle : testé seul, chaque point s'y montre ou s'y cache au
            // pixel près, et le trait se hache d'une image à l'autre. Il reste vu tant qu'il en dépasse
            // d'une demi-largeur, relevée de la hauteur qu'elle couvre à sa distance.
            let pixels_per_world = self.perspective.distance() / self.perspective.linear_depth(depth);
            let lift = self.lift / pixels_per_world / self.perspective.z_per_meter();
            let (raised, raised_depth) = self.perspective.screen_at(point, altitude + lift)?;
            if self.hidden(raised, raised_depth) {
                return None;
            }
        }
        Some([x, y])
    }

    fn camera_zoom(&self) -> f64 {
        self.perspective.camera().zoom
    }
}

/// Morceaux à dessiner : chacun au niveau le plus grossier dont les pixels ne grossissent pas à l'écran,
/// d'autant plus grossier qu'il est loin, à partir de `base` (le niveau de la caméra) ; les niveaux
/// s'emboîtent sans se chevaucher. Du plus proche au plus loin : le GPU écarte tôt ce qui est caché.
fn select_chunks(perspective: &Perspective, travel_zoom: f64, base: u32) -> Vec<ChunkKey> {
    let camera = perspective.camera();
    let top = (base + EXTRA_LEVELS).min((travel_zoom - 1.0).floor().max(0.0) as u32).max(base);
    let footprint: Vec<DVec2> = perspective
        .ground_footprint()
        .iter()
        .map(|&point| perspective.local(point, perspective.ground()).truncate())
        .collect();
    let margin = perspective.max_ground() * 0.05;
    let (min, max) = footprint.iter().fold((DVec2::MAX, DVec2::MIN), |(lo, hi), &p| (lo.min(p), hi.max(p)));
    let (min, max) = (min - margin, max + margin);
    // Morceaux du niveau le plus haut qui touchent l'emprise, puis découpés de proche en proche.
    let target = perspective.target();
    let side = f64::from(CHUNK) * (camera.zoom - (travel_zoom - f64::from(top))).exp2();
    let to_index = |x: f64, y: f64| (((x + target[0]) / side).floor() as i64, ((target[1] - y) / side).floor() as i64);
    let (x0, y0) = to_index(min.x, max.y);
    let (x1, y1) = to_index(max.x, min.y);
    let eye = perspective.eye();
    let distance = perspective.distance();
    let mut out = Vec::new();
    let mut stack: Vec<ChunkKey> =
        (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| ChunkKey { level: top, x, y })).collect();
    while let Some(key) = stack.pop() {
        let rect = local_rect(perspective, travel_zoom, key);
        if !rect.touches(&footprint, margin) {
            continue;
        }
        // Le plus fin des niveaux voulus sur le morceau : à son point le plus proche de la caméra.
        let nearest = rect.nearest(eye.truncate());
        let reach = (nearest - eye.truncate()).length().hypot(eye.z);
        let wanted = base + (reach / distance).max(1.0).log2().floor() as u32;
        if key.level > wanted && key.level > base {
            let (x, y) = (key.x * 2, key.y * 2);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                stack.push(ChunkKey { level: key.level - 1, x: x + dx, y: y + dy });
            }
        } else {
            out.push((reach, key));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.into_iter().map(|(_, key)| key).collect()
}

/// Coin haut gauche (repère local) et côté d'un morceau.
fn local_rect(perspective: &Perspective, travel_zoom: f64, key: ChunkKey) -> LocalRect {
    let camera = perspective.camera();
    let size = f64::from(CHUNK) * (camera.zoom - (travel_zoom - f64::from(key.level))).exp2();
    let target = perspective.target();
    LocalRect { left: key.x as f64 * size - target[0], top: target[1] - key.y as f64 * size, size }
}

/// Un morceau en repère local : x vers l'est, y vers le nord.
struct LocalRect {
    left: f64,
    top: f64,
    size: f64,
}

impl LocalRect {
    fn nearest(&self, point: DVec2) -> DVec2 {
        DVec2::new(point.x.clamp(self.left, self.left + self.size), point.y.clamp(self.top - self.size, self.top))
    }

    /// Vrai si le morceau, élargi de `margin`, touche le polygone convexe `polygon` (axes séparateurs).
    fn touches(&self, polygon: &[DVec2], margin: f64) -> bool {
        let (left, right) = (self.left - margin, self.left + self.size + margin);
        let (bottom, top) = (self.top - self.size - margin, self.top + margin);
        let corners =
            [DVec2::new(left, bottom), DVec2::new(right, bottom), DVec2::new(right, top), DVec2::new(left, top)];
        let separated = |axis: DVec2| {
            let span = |points: &mut dyn Iterator<Item = DVec2>| {
                points.map(|p| p.dot(axis)).fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
            };
            let (a0, a1) = span(&mut corners.iter().copied());
            let (b0, b1) = span(&mut polygon.iter().copied());
            a1 < b0 || b1 < a0
        };
        let edges = (0..polygon.len()).map(|i| polygon[(i + 1) % polygon.len()] - polygon[i]);
        let axes = [DVec2::X, DVec2::Y].into_iter().chain(edges.map(|e| e.perp()));
        !axes.into_iter().any(separated)
    }
}

fn hash(value: impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une caméra inclinée sur l'étape : des morceaux fins près d'elle, sans qu'aucun en recouvre un autre.
    #[test]
    fn nested_levels_coarsen_with_distance_without_overlapping() {
        let travel_zoom = 13.4;
        let camera = Camera { pitch: 55.0, bearing: 300.0, ..Camera::flat([0.0, 43.0], travel_zoom) };
        let perspective = Perspective::new(camera, 1080, 1920, 0.0, 1.5);
        let chunks = select_chunks(&perspective, travel_zoom, 0);
        assert!(chunks.len() > 8 && chunks.len() < 400, "{} morceaux", chunks.len());
        // Le plus proche est au niveau de la caméra ; le brouillard arrête la vue vers 2,4 fois la distance
        // au point visé : au plus un niveau plus grossier au fond.
        assert_eq!(chunks[0].level, 0);
        assert!(chunks.iter().all(|c| c.level <= 1));
        assert!(chunks.iter().filter(|c| c.level == 0).count() * 2 > chunks.len());
        // Aucun morceau n'est l'ancêtre d'un autre.
        let set: BTreeSet<ChunkKey> = chunks.iter().copied().collect();
        assert_eq!(set.len(), chunks.len());
        for key in &chunks {
            let mut parent = *key;
            while parent.level < EXTRA_LEVELS {
                parent = ChunkKey { level: parent.level + 1, x: parent.x.div_euclid(2), y: parent.y.div_euclid(2) };
                assert!(!set.contains(&parent), "{key:?} sous {parent:?}");
            }
        }
        // Le point visé est couvert, au niveau de la caméra.
        let target = perspective.target();
        let side = f64::from(CHUNK);
        let under = ChunkKey { level: 0, x: (target[0] / side).floor() as i64, y: (target[1] / side).floor() as i64 };
        assert!(set.contains(&under));
    }
}
