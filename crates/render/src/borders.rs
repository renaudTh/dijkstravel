//! Frontières `boundary_3` puis `boundary_2` du style liberty d'OpenFreeMap (seules couches gardées du
//! fond vectoriel), tirées des tuiles OpenMapTiles et dessinées dans les morceaux de l'atlas.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use geo_types::Geometry;
use mvt_reader::Reader;
use mvt_reader::feature::Value;
use skia_safe::path_utils::fill_path_with_paint;
use skia_safe::{Canvas, Color, Paint, PathBuilder, PathEffect, Picture, PictureRecorder, Rect, paint};

use crate::cache::Lru;
use crate::mercator::world_size;
use crate::tiles::TileKey;

/// Une frontière : niveau administratif et sommets en fraction de tuile.
struct Border {
    admin_level: i64,
    lines: Vec<Vec<[f32; 2]>>,
}

pub struct Borders {
    color: Color,
    maxzoom: u8,
    dir: PathBuf,
    tiles: Lru<TileKey, Arc<Vec<Border>>>,
    /// Frontières prêtes à rejouer, par tuile et par zoom de niveau (bits du `f64`).
    pictures: Lru<(TileKey, u64), Picture>,
}

/// Opacité imposée par `page.ts` aux deux couches quand le thème montre les frontières.
const OPACITY: f32 = 0.6;
/// `minzoom` de `boundary_3`.
const REGION_MINZOOM: f64 = 5.0;

impl Borders {
    pub fn new(cache_dir: &str, name: &str, maxzoom: u8, color: Color) -> Self {
        Borders {
            color: color.with_a((OPACITY * 255.0).round() as u8),
            maxzoom,
            dir: PathBuf::from(cache_dir).join(name),
            tiles: Lru::new(256),
            pictures: Lru::new(512),
        }
    }

    /// Source vectorielle, tuiles de 512 : `floor(zoom)`, bornée par la source.
    pub fn tile_zoom(&self, level_zoom: f64) -> u8 {
        (level_zoom.floor().max(0.0) as u8).min(self.maxzoom)
    }

    pub fn tiles_for(&self, level_zoom: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<TileKey> {
        let z = self.tile_zoom(level_zoom);
        let tile = world_size(level_zoom) / f64::from(1u32 << z);
        let count = 1i64 << z;
        let mut tiles = Vec::new();
        for y in ((y0 / tile).floor() as i64).max(0)..=((y1 / tile).floor() as i64).min(count - 1) {
            for x in ((x0 / tile).floor() as i64).max(0)..=((x1 / tile).floor() as i64).min(count - 1) {
                tiles.push(TileKey { z, x: x as u32, y: y as u32 });
            }
        }
        tiles
    }

    fn borders(&self, key: TileKey) -> Arc<Vec<Border>> {
        self.tiles.get_or_insert_with(&key, || {
            let file = self.dir.join(key.z.to_string()).join(key.x.to_string()).join(key.y.to_string());
            Arc::new(std::fs::read(file).ok().and_then(decode).unwrap_or_default())
        })
    }

    /// Dessine les frontières de la zone dont le coin est le pixel monde `origin` (zoom `level_zoom`),
    /// chaque tuile limitée à son carré comme le fait MapLibre (sans doublon dans les marges des tuiles).
    pub fn draw(&self, canvas: &Canvas, level_zoom: f64, origin: [f64; 2], width: f64, height: f64) {
        let tile_size = world_size(level_zoom) / f64::from(1u32 << self.tile_zoom(level_zoom));
        for key in self.tiles_for(level_zoom, origin[0], origin[1], origin[0] + width, origin[1] + height) {
            let left = f64::from(key.x) * tile_size - origin[0];
            let top = f64::from(key.y) * tile_size - origin[1];
            let picture = self.picture(key, level_zoom, tile_size);
            canvas.save();
            // Sans anticrénelage : une ligne à cheval sur deux tuiles reste pleine à la jonction.
            canvas.clip_rect(Rect::from_xywh(left as f32, top as f32, tile_size as f32, tile_size as f32), None, false);
            canvas.translate((left as f32, top as f32));
            canvas.draw_picture(&picture, None, None);
            canvas.restore();
        }
    }

    /// Frontières d'une tuile au zoom d'un niveau, traits et pointillés déjà convertis en surfaces : un
    /// morceau d'atlas ne fait que les rejouer, alors qu'il touche plusieurs tuiles et chaque tuile plusieurs morceaux.
    fn picture(&self, key: TileKey, level_zoom: f64, tile_size: f64) -> Picture {
        self.pictures.get_or_insert_with(&(key, level_zoom.to_bits()), || {
            let mut recorder = PictureRecorder::new();
            let bounds = Rect::from_wh(tile_size as f32, tile_size as f32);
            let canvas = recorder.begin_recording(bounds, false);
            let borders = self.borders(key);
            // `boundary_3` puis `boundary_2`, dans l'ordre du style.
            for layer in [Layer::Region, Layer::Country] {
                if matches!(layer, Layer::Region) && level_zoom < REGION_MINZOOM {
                    continue;
                }
                let mut path = PathBuilder::new();
                for border in borders.iter().filter(|b| layer.keeps(b.admin_level)) {
                    for line in &border.lines {
                        let point = |p: &[f32; 2]| (p[0] * tile_size as f32, p[1] * tile_size as f32);
                        let Some((first, rest)) = line.split_first() else { continue };
                        path.move_to(point(first));
                        for p in rest {
                            path.line_to(point(p));
                        }
                    }
                }
                let stroke = layer.paint(self.color, level_zoom);
                let mut filled = PathBuilder::new();
                // Marge : un trait large qui déborde de la tuile reste entier jusqu'à la découpe.
                let cull = bounds.with_outset((8.0, 8.0));
                if fill_path_with_paint(&path.detach(), &stroke, &mut filled, &cull, None) {
                    let mut fill = Paint::default();
                    fill.set_anti_alias(true).set_color(self.color);
                    canvas.draw_path(&filled.detach(), &fill);
                }
            }
            recorder.finish_recording_as_picture(None).expect("frontières enregistrées")
        })
    }
}

#[derive(Clone, Copy)]
enum Layer {
    /// `boundary_3` : niveaux 3 à 6, en pointillés.
    Region,
    /// `boundary_2` : frontières des pays, bouts arrondis.
    Country,
}

impl Layer {
    fn keeps(self, admin_level: i64) -> bool {
        match self {
            Layer::Region => (3..=6).contains(&admin_level),
            Layer::Country => admin_level == 2,
        }
    }

    fn paint(self, color: Color, zoom: f64) -> Paint {
        let mut paint = Paint::default();
        paint.set_anti_alias(true).set_style(paint::Style::Stroke).set_color(color);
        match self {
            Layer::Region => {
                let width = interpolate(zoom, &[(7.0, 1.0), (11.0, 2.0)]) as f32;
                paint.set_stroke_width(width);
                paint.set_path_effect(PathEffect::dash(&[width, width], 0.0));
            }
            Layer::Country => {
                paint.set_stroke_width(interpolate(zoom, &[(3.0, 1.0), (5.0, 1.2), (12.0, 3.0)]) as f32);
                paint.set_stroke_cap(paint::Cap::Round).set_stroke_join(paint::Join::Round);
            }
        }
        paint
    }
}

/// `["interpolate", ["linear"], ["zoom"], …]` : linéaire entre les paliers, constant au-delà.
fn interpolate(zoom: f64, stops: &[(f64, f64)]) -> f64 {
    let (first, last) = (stops[0], stops[stops.len() - 1]);
    if zoom <= first.0 {
        return first.1;
    }
    if zoom >= last.0 {
        return last.1;
    }
    stops
        .windows(2)
        .find(|pair| zoom <= pair[1].0)
        .map(|pair| pair[0].1 + (pair[1].1 - pair[0].1) * (zoom - pair[0].0) / (pair[1].0 - pair[0].0))
        .unwrap_or(last.1)
}

fn number(value: &Value) -> Option<f64> {
    match *value {
        Value::Int(v) | Value::SInt(v) => Some(v as f64),
        Value::UInt(v) => Some(v as f64),
        Value::Float(v) => Some(f64::from(v)),
        Value::Double(v) => Some(v),
        _ => None,
    }
}

/// Filtre des deux couches : frontière terrestre, non disputée, non revendiquée.
fn kept(properties: &HashMap<String, Value>) -> Option<i64> {
    let admin_level = number(properties.get("admin_level")?)?;
    let flag = |name: &str| properties.get(name).and_then(number) == Some(1.0);
    if flag("maritime") || flag("disputed") || properties.contains_key("claimed_by") {
        return None;
    }
    (admin_level.fract() == 0.0).then_some(admin_level as i64)
}

fn decode(bytes: Vec<u8>) -> Option<Vec<Border>> {
    let reader = Reader::new(bytes).ok()?;
    let layer = reader.get_layer_metadata().ok()?.into_iter().find(|layer| layer.name == "boundary")?;
    let extent = layer.extent as f32;
    let features = reader.get_features(layer.layer_index).ok()?;
    let normalize = |c: geo_types::Coord<f32>| [c.x / extent, c.y / extent];
    Some(
        features
            .into_iter()
            .filter_map(|feature| {
                let admin_level = kept(feature.properties.as_ref()?)?;
                let lines: Vec<Vec<[f32; 2]>> = match feature.geometry {
                    Geometry::LineString(line) => vec![line.0.into_iter().map(normalize).collect()],
                    Geometry::MultiLineString(lines) => {
                        lines.0.into_iter().map(|line| line.0.into_iter().map(normalize).collect()).collect()
                    }
                    _ => return None,
                };
                Some(Border { admin_level, lines })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_like_maplibre() {
        let stops = [(3.0, 1.0), (5.0, 1.2), (12.0, 3.0)];
        assert_eq!(interpolate(1.0, &stops), 1.0);
        assert!((interpolate(4.0, &stops) - 1.1).abs() < 1e-12);
        assert_eq!(interpolate(14.0, &stops), 3.0);
    }
}
