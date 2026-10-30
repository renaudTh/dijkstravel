//! Fond de carte : imagerie retouchée, ombrage du relief, voile de couleur et frontières, dessinés une
//! seule fois par morceaux de 512 × 512 pixels monde, puis recadrés à chaque image. Le zoom du parcours
//! est fixe : la plupart des images ne font que translater les mêmes morceaux.

use std::collections::BTreeSet;

use skia_safe::{
    Canvas, Color, ColorFilter, FilterMode, Image, MipmapMode, Paint, Rect, SamplingOptions, canvas::SrcRectConstraint,
    color_filters, surfaces,
};

use crate::borders::Borders;
use crate::cache::Lru;
use crate::color::parse_color;
use crate::frame::Camera;
use crate::hillshade::Hillshade;
use crate::mercator::{View, world_size};
use crate::model::{BasemapTheme, ImagerySource, Input, TileSource};
use crate::tiles::{TileKey, TileStore};

const CHUNK: i32 = 512;
/// Bordure dessinée en plus autour d'un morceau : le filtrage bilinéaire lit les pixels voisins.
const APRON: i32 = 2;
/// Morceaux gardés : 1 Mo chacun.
const CHUNKS: usize = 384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkKey {
    /// Niveau 0 : zoom du parcours ; niveau n : n zooms en dessous (intro et outro).
    pub level: u32,
    pub x: i64,
    pub y: i64,
}

pub struct Atlas {
    /// Ce dont dépend tout l'atlas : un aperçu qui ne change rien de tout ça le reprend tel quel.
    depends_on: (u64, BasemapTheme, ImagerySource, TileSource, TileSource, String),
    travel_zoom: f64,
    imagery: TileStore,
    hillshade: Hillshade,
    borders: Option<Borders>,
    adjust: ColorFilter,
    tint: Color,
    background: Color,
    chunks: Lru<ChunkKey, Image>,
}

/// Un morceau à dessiner : où le prendre dans l'atlas, où le poser à l'écran.
struct Placement {
    key: ChunkKey,
    dest: Rect,
}

impl Atlas {
    pub fn new(travel_zoom: f64, input: &Input) -> Self {
        let basemap = &input.theme.basemap;
        let (dir, terrain, vector) = (&input.tile_dir, &input.terrain, &input.vector);
        Atlas {
            depends_on: (
                travel_zoom.to_bits(),
                basemap.clone(),
                input.imagery.clone(),
                terrain.clone(),
                vector.clone(),
                input.tile_dir.clone(),
            ),
            travel_zoom,
            imagery: TileStore::new(dir, &input.imagery.name, input.imagery.maxzoom),
            hillshade: Hillshade::new(dir, &terrain.name, terrain.maxzoom, basemap.hillshade),
            borders: basemap
                .borders
                .then(|| Borders::new(dir, &vector.name, vector.maxzoom, parse_color(&basemap.border_color))),
            adjust: raster_adjust(basemap.saturation, basemap.contrast, basemap.brightness),
            tint: parse_color(&basemap.tint_color).with_a((basemap.tint_opacity.clamp(0.0, 1.0) * 255.0).round() as u8),
            background: parse_color(&basemap.background_color),
            chunks: Lru::new(CHUNKS),
        }
    }

    /// Vrai si cet atlas est celui qu'aurait ce rendu : même zoom, même fond, mêmes sources.
    pub fn serves(&self, travel_zoom: f64, input: &Input) -> bool {
        let (zoom, basemap, imagery, terrain, vector, dir) = &self.depends_on;
        *zoom == travel_zoom.to_bits()
            && *basemap == input.theme.basemap
            && *imagery == input.imagery
            && *terrain == input.terrain
            && *vector == input.vector
            && *dir == input.tile_dir
    }

    fn level_zoom(&self, level: u32) -> f64 {
        self.travel_zoom - f64::from(level)
    }

    /// Niveau au zoom égal ou juste au-dessus : l'image ne fait que réduire, jamais agrandir.
    fn level_for(&self, zoom: f64) -> u32 {
        (self.travel_zoom - zoom + 1e-9).floor().max(0.0) as u32
    }

    /// Repère de l'image : celui du niveau de l'atlas qui la dessine.
    pub fn view(&self, camera: Camera, width: u32, height: u32) -> View {
        let level = self.level_for(camera.zoom);
        View::new(camera.center, camera.zoom, self.level_zoom(level), width, height)
    }

    fn placements(&self, view: View, width: u32, height: u32) -> Vec<Placement> {
        let level = (self.travel_zoom - view.zoom).round().max(0.0) as u32;
        let scale = view.scale;
        let [ox, oy] = view.origin;
        let (span_w, span_h) = (f64::from(width) / scale, f64::from(height) / scale);
        let chunk = f64::from(CHUNK);
        let range = |from: f64, to: f64| (from / chunk).floor() as i64..=(to / chunk).floor() as i64;
        let mut placements = Vec::new();
        for y in range(oy, oy + span_h) {
            for x in range(ox, ox + span_w) {
                let left = (x as f64 * chunk - ox) * scale;
                let top = (y as f64 * chunk - oy) * scale;
                let size = chunk * scale;
                placements.push(Placement {
                    key: ChunkKey { level, x, y },
                    dest: Rect::from_xywh(left as f32, top as f32, size as f32, size as f32),
                });
            }
        }
        placements
    }

    /// Morceaux vus par une caméra : pour lister les tuiles à télécharger avant le rendu.
    pub fn chunks_for(&self, camera: Camera, width: u32, height: u32) -> Vec<ChunkKey> {
        self.placements(self.view(camera, width, height), width, height).into_iter().map(|p| p.key).collect()
    }

    pub fn draw(&self, canvas: &Canvas, view: View, width: u32, height: u32) {
        let placements = self.placements(view, width, height);
        if view.scale == 1.0 && self.copy(canvas, &placements) {
            return;
        }
        // Morceaux opaques qui couvrent tout l'écran : posés sans mélange.
        let mut paint = Paint::default();
        paint.set_blend_mode(skia_safe::BlendMode::Src);
        let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear);
        let source = Rect::from_xywh(APRON as f32, APRON as f32, CHUNK as f32, CHUNK as f32);
        for Placement { key, dest } in placements {
            let image = self.chunk(key);
            let constraint = Some((&source, SrcRectConstraint::Fast));
            canvas.draw_image_rect_with_sampling_options(&image, constraint, dest, sampling, &paint);
        }
    }

    /// Échelle 1, coins entiers : les lignes des morceaux sont recopiées telles quelles dans la surface,
    /// bien plus vite que par le dessin de Skia. `false` si les pixels ne sont pas accessibles.
    fn copy(&self, canvas: &Canvas, placements: &[Placement]) -> bool {
        let Some(target) = canvas.access_top_layer_pixels() else { return false };
        let (width, height) = (target.info.width(), target.info.height());
        let color_type = target.info.color_type();
        for placement in placements {
            let image = self.chunk(placement.key);
            let Some(source) = image.peek_pixels() else { return false };
            let Some(bytes) = source.bytes() else { return false };
            if source.color_type() != color_type {
                return false;
            }
            let (left, top) = (placement.dest.left as i32, placement.dest.top as i32);
            let (x0, x1) = (left.max(0), (left + CHUNK).min(width));
            let (y0, y1) = (top.max(0), (top + CHUNK).min(height));
            if x0 >= x1 || y0 >= y1 {
                continue;
            }
            let row = (x1 - x0) as usize * 4;
            for y in y0..y1 {
                let from = (y - top + APRON) as usize * source.row_bytes() + (x0 - left + APRON) as usize * 4;
                let to = y as usize * target.row_bytes + x0 as usize * 4;
                target.pixels[to..to + row].copy_from_slice(&bytes[from..from + row]);
            }
        }
        true
    }

    /// Zoom des tuiles raster de MapLibre (`coveringZoomLevel`, arrondi, tuiles de 256), borné par la source.
    fn tile_zoom(&self, level: u32) -> u8 {
        ((self.level_zoom(level) + 1.0).round().max(0.0) as u8).min(self.imagery.maxzoom)
    }

    /// Pixels monde (au zoom du niveau) que couvre un morceau, bordure comprise : `[x0, y0, x1, y1]`.
    fn bounds(key: ChunkKey) -> [f64; 4] {
        let (chunk, apron) = (i64::from(CHUNK), i64::from(APRON));
        let x0 = (key.x * chunk - apron) as f64;
        let y0 = (key.y * chunk - apron) as f64;
        [x0, y0, x0 + f64::from(CHUNK + 2 * APRON), y0 + f64::from(CHUNK + 2 * APRON)]
    }

    fn imagery_tiles(&self, key: ChunkKey) -> Vec<TileKey> {
        let z = self.tile_zoom(key.level);
        let tile = world_size(self.level_zoom(key.level)) / f64::from(1u32 << z);
        let count = 1i64 << z;
        let [x0, y0, x1, y1] = Self::bounds(key);
        let mut tiles = Vec::new();
        for y in ((y0 / tile).floor() as i64).max(0)..=((y1 / tile).ceil() as i64 - 1).min(count - 1) {
            for x in ((x0 / tile).floor() as i64).max(0)..=((x1 / tile).ceil() as i64 - 1).min(count - 1) {
                tiles.push(TileKey { z, x: x as u32, y: y as u32 });
            }
        }
        tiles
    }

    /// Tuiles de toutes les sources que lit un morceau, `<source>/<z>/<x>/<y>`.
    pub fn tiles_of(&self, key: ChunkKey) -> BTreeSet<String> {
        let name = |source: &str, t: TileKey| format!("{source}/{}/{}/{}", t.z, t.x, t.y);
        let zoom = self.level_zoom(key.level);
        let [x0, y0, x1, y1] = Self::bounds(key);
        let mut tiles: BTreeSet<String> =
            self.imagery_tiles(key).into_iter().map(|t| name(&self.imagery.name, t)).collect();
        if self.hillshade.enabled() {
            tiles.extend(self.hillshade.tiles_for(zoom, x0, y0, x1, y1).into_iter().map(|t| name("terrarium", t)));
        }
        if let Some(borders) = &self.borders {
            tiles.extend(borders.tiles_for(zoom, x0, y0, x1, y1).into_iter().map(|t| name("openmaptiles", t)));
        }
        tiles
    }

    fn chunk(&self, key: ChunkKey) -> Image {
        self.chunks.get_or_insert_with(&key, || self.render_chunk(key))
    }

    /// Ordre des couches de `page.ts` : imagerie retouchée, ombrage, voile, frontières.
    fn render_chunk(&self, key: ChunkKey) -> Image {
        let side = CHUNK + 2 * APRON;
        let mut surface = surfaces::raster_n32_premul((side, side)).expect("surface du morceau");
        let canvas = surface.canvas();
        // Hors couverture de l'imagerie (IGN au large) : le fond du thème, blanc comme la page par défaut.
        canvas.clear(self.background);
        let zoom = self.level_zoom(key.level);
        let [x0, y0, ..] = Self::bounds(key);
        let tile_size = world_size(zoom) / f64::from(1u32 << self.tile_zoom(key.level));
        let mut paint = Paint::default();
        paint.set_color_filter(self.adjust.clone());
        // Comme MapLibre : bilinéaire, mipmap la plus proche en réduction (`LINEAR_MIPMAP_NEAREST`).
        let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::Nearest);
        for tile in self.imagery_tiles(key) {
            let Some(image) = self.imagery.get(tile) else { continue };
            let left = f64::from(tile.x) * tile_size - x0;
            let top = f64::from(tile.y) * tile_size - y0;
            let dest = Rect::from_xywh(left as f32, top as f32, tile_size as f32, tile_size as f32);
            canvas.draw_image_rect_with_sampling_options(&image, None, dest, sampling, &paint);
        }
        if self.hillshade.enabled()
            && let Some(shade) = self.hillshade.render(zoom, [x0, y0], side as usize, side as usize)
        {
            canvas.draw_image(&shade, (0, 0), None);
        }
        if self.tint.a() > 0 {
            canvas.draw_color(self.tint, None);
        }
        if let Some(borders) = &self.borders {
            borders.draw(canvas, zoom, [x0, y0], f64::from(side), f64::from(side));
        }
        let image = surface.image_snapshot();
        if key.level == 0 { image } else { image.with_default_mipmaps().unwrap_or(image) }
    }
}

/// Retouches `raster-saturation`, `raster-contrast` et `raster-brightness-min` du shader raster de
/// MapLibre : toutes affines par canal, donc une seule matrice de couleur.
fn raster_adjust(saturation: f64, contrast: f64, brightness_min: f64) -> ColorFilter {
    let saturation_factor = if saturation > 0.0 { 1.0 - 1.0 / (1.001 - saturation) } else { -saturation };
    let contrast_factor = if contrast > 0.0 { 1.0 / (1.0 - contrast) } else { 1.0 + contrast };
    // rgb + (moyenne - rgb) × s, puis (c - 0,5) × k + 0,5, puis min + (1 - min) × c.
    let gain = (1.0 - brightness_min) * contrast_factor;
    let diagonal = (gain * (1.0 - saturation_factor + saturation_factor / 3.0)) as f32;
    let cross = (gain * saturation_factor / 3.0) as f32;
    let offset = ((1.0 - brightness_min) * 0.5 * (1.0 - contrast_factor) + brightness_min) as f32;
    #[rustfmt::skip]
    let matrix = [
        diagonal, cross, cross, 0.0, offset,
        cross, diagonal, cross, 0.0, offset,
        cross, cross, diagonal, 0.0, offset,
        0.0, 0.0, 0.0, 1.0, 0.0,
    ];
    color_filters::matrix_row_major(&matrix, None)
}
