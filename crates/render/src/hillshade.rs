//! Ombrage du relief, portage de la couche `hillshade` de MapLibre (méthode `standard`) : préparation
//! des pentes par tuile de relief (shader `hillshadePrepare`), puis couleur de chaque pixel (shader
//! `hillshade`). Calculé une fois par morceau d'atlas, au lieu d'une passe plein écran par image.

use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

use rayon::prelude::*;
use skia_safe::{AlphaType, ColorType, Data, Image, ImageInfo};

use crate::cache::Lru;
use crate::elevation::{DIM, Elevation};
use crate::mercator::{unproject, world_size};
use crate::tiles::TileKey;

/// Zoom de relief le plus fin de l'ombrage, celui de l'ancien rendu MapLibre : au-delà, seule la vue
/// inclinée lit des tuiles plus fines.
const MAXZOOM: u8 = 12;
/// Grille préparée : la tuile et une bordure d'un pixel prise sur ses voisines.
const PREPARED: usize = DIM + 2;
/// `u_log2_world_width` de MapLibre pour Web Mercator.
const LOG2_WORLD_WIDTH: f64 = 25.2562;
/// Couleurs d'ombre et de lumière de l'ancien rendu MapLibre, accent noir par défaut.
const SHADOW: [f32; 3] = [0x1f as f32 / 255.0, 0x2a as f32 / 255.0, 0x1a as f32 / 255.0];
const HIGHLIGHT: [f32; 3] = [1.0, 0xf7 as f32 / 255.0, 0xe0 as f32 / 255.0];
/// `hillshade-illumination-direction` par défaut (335°), plus π comme dans le shader.
const AZIMUTH: f32 = 335.0 * PI / 180.0 + PI;

/// Pixels de relief agrandis au plus sur ce nombre de pixels de l'ombrage calculé : au-delà, il est calculé
/// sur une grille plus lâche (un pixel sur 2, 4 ou 8) puis agrandi. Les pentes varient bilinéairement
/// dans un pixel de relief ; échantillonnées deux fois par pixel au moins, l'écart reste invisible.
const SAMPLES_PER_DEM_PIXEL: f64 = 2.0;
const MAX_STEP: usize = 8;

/// Pentes quantifiées sur 8 bits par canal, comme la texture que remplit `hillshadePrepare`.
type Prepared = Vec<[u8; 2]>;

pub struct Hillshade {
    exaggeration: f64,
    maxzoom: u8,
    elevation: Arc<Elevation>,
    prepared: Lru<TileKey, Option<Arc<Prepared>>>,
}

impl Hillshade {
    pub fn new(elevation: Arc<Elevation>, exaggeration: f64) -> Self {
        Hillshade { exaggeration, maxzoom: elevation.maxzoom.min(MAXZOOM), elevation, prepared: Lru::new(256) }
    }

    pub fn enabled(&self) -> bool {
        self.exaggeration > 0.0
    }

    /// Zoom des tuiles de relief pour un niveau d'atlas : source raster-dem, tuiles de 256, arrondi.
    pub fn tile_zoom(&self, level_zoom: f64) -> u8 {
        ((level_zoom + 1.0).round().max(0.0) as u8).min(self.maxzoom)
    }

    /// Tuiles de relief lues pour couvrir `[x0, x1[ × [y0, y1[` (pixels monde au zoom `level_zoom`),
    /// voisines comprises : la bordure de chaque tuile est prise sur elles.
    pub fn tiles_for(&self, level_zoom: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<TileKey> {
        self.elevation.tiles_covering(self.tile_zoom(level_zoom), level_zoom, [x0, y0, x1, y1], 1)
    }

    fn elevations(&self, key: TileKey) -> Option<Arc<Vec<f32>>> {
        self.elevation.tile(key)
    }

    fn prepared(&self, key: TileKey) -> Option<Arc<Prepared>> {
        self.prepared.get_or_insert_with(&key, || {
            let own = self.elevations(key)?;
            let count = 1i64 << key.z;
            let neighbor = |dx: i64, dy: i64| {
                let y = i64::from(key.y) + dy;
                if !(0..count).contains(&y) {
                    return None;
                }
                let x = (i64::from(key.x) + dx).rem_euclid(count);
                self.elevations(TileKey { z: key.z, x: x as u32, y: y as u32 })
            };
            let neighbors: Vec<Option<Arc<Vec<f32>>>> =
                (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (dx, dy))).map(|(dx, dy)| neighbor(dx, dy)).collect();
            let dim = DIM as i64;
            // Altitudes de la tuile et de deux pixels de ses voisines, lues une fois ; une voisine absente
            // prolonge le bord.
            const EXT: usize = DIM + 4;
            let mut grid = vec![0f32; EXT * EXT];
            for (i, value) in grid.iter_mut().enumerate() {
                let (x, y) = ((i % EXT) as i64 - 2, (i / EXT) as i64 - 2);
                let (dx, dy) = (x.div_euclid(dim), y.div_euclid(dim));
                *value = match &neighbors[((dy + 1) * 3 + dx + 1) as usize] {
                    Some(tile) => tile[(y.rem_euclid(dim) * dim + x.rem_euclid(dim)) as usize],
                    None => own[(y.clamp(0, dim - 1) * dim + x.clamp(0, dim - 1)) as usize],
                };
            }
            let z = f64::from(key.z);
            let factor = if z < 2.0 {
                0.4
            } else if z < 4.5 {
                0.35
            } else {
                0.3
            };
            let exaggeration = if z < 15.0 { (z - 15.0) * factor } else { 0.0 };
            let scale = (DIM as f64 / (exaggeration + 3.0 + LOG2_WORLD_WIDTH - z).exp2()) as f32;
            let quantize = |v: f32| ((v / 8.0 + 128.0 / 255.0).clamp(0.0, 1.0) * 255.0).round() as u8;
            let mut prepared = vec![[0u8; 2]; PREPARED * PREPARED];
            for py in 0..PREPARED {
                for px in 0..PREPARED {
                    // Texel (px, py) : pixel (px − 1, py − 1) de la tuile, soit (px + 1, py + 1) dans la grille.
                    let e = |dx: usize, dy: usize| grid[(py + dy) * EXT + px + dx];
                    let (a, b, c) = (e(0, 0), e(1, 0), e(2, 0));
                    let (d, f) = (e(0, 1), e(2, 1));
                    let (g, h, i) = (e(0, 2), e(1, 2), e(2, 2));
                    let deriv_x = ((c + f + f + i) - (a + d + d + g)) * scale;
                    let deriv_y = ((g + h + h + i) - (a + b + b + c)) * scale;
                    prepared[py * PREPARED + px] = [quantize(deriv_x), quantize(deriv_y)];
                }
            }
            Some(Arc::new(prepared))
        })
    }

    /// Ombrage d'une zone `width × height` dont le coin est le pixel monde `origin` (zoom `level_zoom`),
    /// en pixels RGBA prémultipliés, à poser par-dessus l'imagerie.
    pub fn render(&self, level_zoom: f64, origin: [f64; 2], width: usize, height: usize) -> Option<Image> {
        let z = self.tile_zoom(level_zoom);
        let tiles_at_zoom = f64::from(1u32 << z);
        // Pixels de tuile de relief par pixel monde.
        let to_dem = tiles_at_zoom * DIM as f64 / world_size(level_zoom);
        let shading = Shading::new(self.exaggeration as f32);
        // Au-delà du zoom de relief le plus fin, un pixel de relief couvre des dizaines de pixels.
        let mut step = 1;
        while step < MAX_STEP && (2 * step) as f64 * to_dem * SAMPLES_PER_DEM_PIXEL <= 1.0 {
            step *= 2;
        }
        let (columns, rows) = (width.div_ceil(step), height.div_ceil(step));
        // Centre du pixel `k` de la grille, en pixels monde depuis le coin.
        let center = |k: usize| (k * step) as f64 + step as f64 / 2.0;
        let mut pixels = vec![0u8; columns * rows * 4];
        // Lignes réparties sur les cœurs : un morceau d'atlas compte 266 000 pixels.
        pixels.par_chunks_mut(columns * 4).enumerate().for_each(|(row, out)| {
            let dem_y = (origin[1] + center(row)) * to_dem;
            let mut current: Option<(TileKey, Option<Arc<Prepared>>, f32)> = None;
            for column in 0..columns {
                let dem_x = (origin[0] + center(column)) * to_dem;
                let key = TileKey {
                    z,
                    x: ((dem_x / DIM as f64).floor() as i64).rem_euclid(1 << z) as u32,
                    y: ((dem_y / DIM as f64).floor().max(0.0) as u32).min((1u32 << z) - 1),
                };
                if current.as_ref().is_none_or(|(k, _, _)| *k != key) {
                    let [lat_top, lat_bottom] = tile_lat_range(key);
                    let v = dem_y - f64::from(key.y) * DIM as f64;
                    // Latitude interpolée dans la tuile, comme le shader ; constante sur la ligne.
                    let lat = (lat_top - lat_bottom) * (1.0 - v / DIM as f64) + lat_bottom;
                    current = Some((key, self.prepared(key), (8.0 / lat.to_radians().cos()) as f32));
                }
                let Some((_, Some(prepared), deriv_scale)) = current.as_ref() else { continue };
                let u = dem_x - (dem_x / DIM as f64).floor() * DIM as f64;
                let v = dem_y - f64::from(key.y) * DIM as f64;
                // Texel k de la grille préparée : pixel k − 1 de la tuile, à cause de la bordure.
                let [r, g] = bilinear(prepared, (u + 1.0) as f32, (v + 1.0) as f32);
                let deriv = [(r - 128.0 / 255.0) * deriv_scale, (g - 128.0 / 255.0) * deriv_scale];
                let color = shading.color(deriv);
                for (channel, value) in out[column * 4..column * 4 + 4].iter_mut().zip(color) {
                    *channel = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
        });
        let info = ImageInfo::new((columns as i32, rows as i32), ColorType::RGBA8888, AlphaType::Premul, None);
        let grid = skia_safe::images::raster_from_data(&info, Data::new_copy(&pixels), columns * 4)?;
        if step == 1 {
            return Some(grid);
        }
        // Agrandie en bilinéaire : chaque pixel de la grille couvre `step × step` pixels, centrés.
        let mut surface = skia_safe::surfaces::raster_n32_premul((width as i32, height as i32))?;
        let dest = skia_safe::Rect::from_wh((columns * step) as f32, (rows * step) as f32);
        let sampling = skia_safe::SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::None);
        let mut paint = skia_safe::Paint::default();
        paint.set_blend_mode(skia_safe::BlendMode::Src);
        surface.canvas().draw_image_rect_with_sampling_options(&grid, None, dest, sampling, &paint);
        Some(surface.image_snapshot())
    }
}

/// Latitudes des bords haut et bas d'une tuile.
fn tile_lat_range(key: TileKey) -> [f64; 2] {
    let size = f64::from(1u32 << key.z);
    let lat = |y: f64| unproject([0.0, y / size * 512.0], 0.0)[1];
    [lat(f64::from(key.y)), lat(f64::from(key.y) + 1.0)]
}

/// Bilinéaire sur la grille préparée ; `(x, y)` en texels, centres en `k + 0,5` (bordure comprise).
fn bilinear(prepared: &Prepared, x: f32, y: f32) -> [f32; 2] {
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let at = |x: f32, y: f32| {
        let (x, y) = ((x.max(0.0) as usize).min(PREPARED - 1), (y.max(0.0) as usize).min(PREPARED - 1));
        prepared[y * PREPARED + x]
    };
    let (a, b, c, d) = (at(x0, y0), at(x0 + 1.0, y0), at(x0, y0 + 1.0), at(x0 + 1.0, y0 + 1.0));
    let mix = |i: usize| {
        let top = f32::from(a[i]) * (1.0 - tx) + f32::from(b[i]) * tx;
        let bottom = f32::from(c[i]) * (1.0 - tx) + f32::from(d[i]) * tx;
        (top * (1.0 - ty) + bottom * ty) / 255.0
    };
    [mix(0), mix(1)]
}

/// `standard_hillshade` du shader de MapLibre, constantes calculées une fois.
struct Shading {
    intensity: f32,
    strength: f32,
    ln_base: f32,
    /// `base^(π/2) − 1`.
    max_scaled: f32,
}

impl Shading {
    fn new(intensity: f32) -> Self {
        let base = 1.875 - intensity * 1.75;
        Shading {
            intensity,
            strength: (intensity * 2.0).clamp(0.0, 1.0),
            ln_base: base.ln(),
            max_scaled: base.powf(FRAC_PI_2) - 1.0,
        }
    }

    /// RGBA prémultiplié.
    fn color(&self, deriv: [f32; 2]) -> [f32; 4] {
        let slope = (0.625 * deriv[0].hypot(deriv[1])).atan();
        let aspect = if deriv[0] != 0.0 {
            deriv[1].atan2(-deriv[0])
        } else {
            FRAC_PI_2 * if deriv[1] > 0.0 { 1.0 } else { -1.0 }
        };
        let scaled_slope = if self.intensity != 0.5 {
            ((slope * self.ln_base).exp() - 1.0) / self.max_scaled * FRAC_PI_2
        } else {
            slope
        };
        let (sin, cos) = scaled_slope.sin_cos();
        // Accent noir prémultiplié : seule son alpha compte.
        let accent_alpha = (1.0 - cos) * self.strength;
        let shade = (glsl_mod((aspect + AZIMUTH) / PI + 0.5, 2.0) - 1.0).abs();
        let weight = sin * self.strength;
        let rgb = [0, 1, 2].map(|i| (SHADOW[i] + (HIGHLIGHT[i] - SHADOW[i]) * shade) * weight);
        [rgb[0], rgb[1], rgb[2], accent_alpha * (1.0 - weight) + weight]
    }
}

fn glsl_mod(x: f32, y: f32) -> f32 {
    x - y * (x / y).floor()
}
