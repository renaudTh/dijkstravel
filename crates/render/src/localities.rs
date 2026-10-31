//! Localités traversées : villes et villages de la couche `place` des tuiles OpenMapTiles près du tracé.
//! Leur nom apparaît à leur place sur la carte quand la tête approche et s'efface une fois passé, deux au
//! plus à la fois, les plus importants d'abord.

use std::collections::BTreeSet;
use std::path::PathBuf;

use mvt_reader::Reader;
use mvt_reader::feature::Value;
use skia_safe::{Canvas, Color, Paint, Rect};

use crate::geo::{LonLat, MeasuredPath, distance_meters};
use crate::mercator::{Projection, project, unproject};
use crate::model::Tour;
use crate::sprites::{Fonts, Label};
use crate::tiles::TileKey;

/// Zoom des tuiles lues : villes et villages y sont tous.
pub const ZOOM: u8 = 12;
/// Classes gardées, de la plus importante à la moins : distance au tracé (mètres) en deçà de laquelle la
/// localité est traversée.
const CLASSES: [(&str, f64); 3] = [("city", 2500.0), ("town", 1200.0), ("village", 600.0)];
/// Le nom apparaît quand la tête est à cette part d'écran de la localité, et reste jusqu'à cette part
/// après.
const LEAD_SCREENS: f64 = 0.8;
const TRAIL_SCREENS: f64 = 0.5;
/// Part de sa présence sur laquelle le nom apparaît, puis s'efface.
const FADE_SHARE: f64 = 0.25;
/// Noms à la fois au plus.
const AT_ONCE: usize = 2;
/// Une localité à moins de cette distance d'un lieu de la tournée porte déjà son nom.
const NEAR_STOP: f64 = 1500.0;
/// Pas de la recherche des tuiles le long du tracé, en mètres.
const SAMPLE_METERS: f64 = 1000.0;

struct Locality {
    at: LonLat,
    /// Mètres de tracé où le nom apparaît puis disparaît.
    window: [f64; 2],
    label: Label,
}

pub struct Localities {
    localities: Vec<Locality>,
}

/// Une localité lue dans les tuiles : nom, rang de sa classe dans `CLASSES`, position.
struct Place {
    name: String,
    class: usize,
    at: LonLat,
}

/// Tuiles `<source>/<z>/<x>/<y>` qui couvrent le tracé et la plus grande distance de `CLASSES`.
pub fn tiles(path: &MeasuredPath, source: &str) -> BTreeSet<String> {
    tile_keys(path).into_iter().map(|t| format!("{source}/{}/{}/{}", t.z, t.x, t.y)).collect()
}

fn tile_keys(path: &MeasuredPath) -> BTreeSet<TileKey> {
    let reach = CLASSES.iter().map(|&(_, reach)| reach).fold(0.0, f64::max);
    let total = path.total_meters();
    let count = (total / SAMPLE_METERS).ceil() as usize + 1;
    let side = 1u32 << ZOOM;
    let mut keys = BTreeSet::new();
    for i in 0..count {
        let [lon, lat] = path.point_at(total * i as f64 / (count - 1).max(1) as f64);
        let dlat = reach / 111_320.0;
        let dlon = dlat / lat.to_radians().cos().max(0.01);
        let [x0, y0] = project([lon - dlon, lat + dlat], f64::from(ZOOM));
        let [x1, y1] = project([lon + dlon, lat - dlat], f64::from(ZOOM));
        for y in (y0 / 512.0).floor() as i64..=(y1 / 512.0).floor() as i64 {
            for x in (x0 / 512.0).floor() as i64..=(x1 / 512.0).floor() as i64 {
                if (0..i64::from(side)).contains(&y) {
                    keys.insert(TileKey { z: ZOOM, x: x.rem_euclid(i64::from(side)) as u32, y: y as u32 });
                }
            }
        }
    }
    keys
}

impl Localities {
    /// `screen_meters` : mètres au sol sur le petit côté de l'écran pendant le trajet.
    pub fn new(
        cache_dir: &str,
        source: &str,
        tour: &Tour,
        fonts: &Fonts,
        size: f32,
        [text, halo]: [Color; 2],
        screen_meters: f64,
    ) -> Self {
        let dir = PathBuf::from(cache_dir).join(source);
        let mut places: Vec<Place> = Vec::new();
        for key in tile_keys(&tour.path) {
            let file = dir.join(key.z.to_string()).join(key.x.to_string()).join(key.y.to_string());
            for place in std::fs::read(file).ok().and_then(|bytes| decode(bytes, key)).unwrap_or_default() {
                // Une localité se répète dans la marge des tuiles voisines.
                if !places.iter().any(|other| other.name == place.name) {
                    places.push(place);
                }
            }
        }
        let crossed: Vec<(usize, f64, Place)> = places
            .into_iter()
            .filter(|place| {
                !tour.stops.iter().any(|stop| {
                    distance_meters(stop.at, place.at) < NEAR_STOP || stop.name.contains(place.name.as_str())
                })
            })
            .filter_map(|place| {
                let (distance, meters) = nearest(&tour.path, place.at);
                (distance <= CLASSES[place.class].1).then_some((place.class, meters, place))
            })
            .collect();
        let windows: Vec<[f64; 2]> = crossed
            .iter()
            .map(|&(_, meters, _)| [meters - LEAD_SCREENS * screen_meters, meters + TRAIL_SCREENS * screen_meters])
            .collect();
        // Les plus importantes d'abord, puis dans l'ordre du tracé : une localité n'est gardée que si
        // moins de `AT_ONCE` noms déjà gardés se montrent en même temps qu'elle.
        let mut order: Vec<usize> = (0..crossed.len()).collect();
        order.sort_by(|&a, &b| crossed[a].0.cmp(&crossed[b].0).then(crossed[a].1.total_cmp(&crossed[b].1)));
        let mut kept: Vec<usize> = Vec::new();
        for i in order {
            let [from, to] = windows[i];
            let overlapping = kept.iter().filter(|&&k| windows[k][0] < to && from < windows[k][1]).count();
            if overlapping < AT_ONCE {
                kept.push(i);
            }
        }
        let localities = kept
            .into_iter()
            .filter_map(|i| {
                let label = fonts.label(&crossed[i].2.name, size, text, halo)?;
                Some(Locality { at: crossed[i].2.at, window: windows[i], label })
            })
            .collect();
        Localities { localities }
    }

    /// Les noms des localités dont la tête est proche, centrés sur elles.
    pub fn draw(&self, canvas: &Canvas, view: &dyn Projection, screen: Rect, meters: f64, opacity: f32) {
        for locality in &self.localities {
            let [from, to] = locality.window;
            if meters <= from || meters >= to {
                continue;
            }
            let t = (meters - from) / (to - from);
            let fade = smoothstep((t / FADE_SHARE).min(1.0)) * smoothstep(((1.0 - t) / FADE_SHARE).min(1.0));
            let Some([x, y]) = view.screen_point(locality.at) else { continue };
            let label = &locality.label;
            let spot =
                Rect::from_xywh(x as f32 - label.width / 2.0, y as f32 - label.height / 2.0, label.width, label.height);
            if !screen.intersects(spot) {
                continue;
            }
            let mut paint = Paint::default();
            paint.set_alpha_f(fade as f32 * opacity);
            canvas.draw_image(&label.image, (spot.left - label.pad, spot.top - label.pad), Some(&paint));
        }
    }
}

/// Distance du point au tracé, en mètres, et distance le long du tracé du point le plus proche.
fn nearest(path: &MeasuredPath, at: LonLat) -> (f64, f64) {
    let scale = at[1].to_radians().cos();
    let local = |p: LonLat| [(p[0] - at[0]) * scale * 111_320.0, (p[1] - at[1]) * 111_320.0];
    let mut best = (f64::INFINITY, 0.0);
    for i in 0..path.points.len().saturating_sub(1) {
        let (a, b) = (local(path.points[i]), local(path.points[i + 1]));
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = dx * dx + dy * dy;
        let t = if length > 0.0 { (-(a[0] * dx + a[1] * dy) / length).clamp(0.0, 1.0) } else { 0.0 };
        let distance = (a[0] + dx * t).hypot(a[1] + dy * t);
        if distance < best.0 {
            best = (distance, path.meters[i] + (path.meters[i + 1] - path.meters[i]) * t);
        }
    }
    best
}

fn text(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) => Some(text.as_str()),
        _ => None,
    }
}

fn decode(bytes: Vec<u8>, key: TileKey) -> Option<Vec<Place>> {
    let reader = Reader::new(bytes).ok()?;
    let layer = reader.get_layer_metadata().ok()?.into_iter().find(|layer| layer.name == "place")?;
    let extent = f64::from(layer.extent);
    let features = reader.get_features(layer.layer_index).ok()?;
    let places = features
        .into_iter()
        .filter_map(|feature| {
            let properties = feature.properties.as_ref()?;
            let class = text(properties.get("class")?)?;
            let class = CLASSES.iter().position(|&(name, _)| name == class)?;
            let name = text(properties.get("name")?)?.trim().to_owned();
            // Les points arrivent en multipoints d'un seul point.
            let point = match feature.geometry {
                geo_types::Geometry::Point(point) => point,
                geo_types::Geometry::MultiPoint(points) => *points.0.first()?,
                _ => return None,
            };
            let world = [
                (f64::from(key.x) + f64::from(point.x()) / extent) * 512.0,
                (f64::from(key.y) + f64::from(point.y()) / extent) * 512.0,
            ];
            (!name.is_empty()).then(|| Place { name, class, at: unproject(world, f64::from(ZOOM)) })
        })
        .collect();
    Some(places)
}

fn smoothstep(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_nearest_point_along_the_route() {
        let path = MeasuredPath::measure(vec![[6.0, 46.0], [6.1, 46.0]]);
        // À 0,05° de longitude du départ (≈ 3,9 km), 500 m au nord.
        let (distance, meters) = nearest(&path, [6.05, 46.0 + 500.0 / 111_320.0]);
        assert!((distance - 500.0).abs() < 1.0, "{distance}");
        assert!((meters - path.total_meters() / 2.0).abs() < 20.0, "{meters}");
    }

    #[test]
    fn covers_the_route_corridor_with_tiles() {
        let path = MeasuredPath::measure(vec![[6.47, 46.39], [6.6, 46.37]]);
        let keys = tile_keys(&path);
        assert!(!keys.is_empty() && keys.len() < 20, "{} tuiles", keys.len());
        assert!(keys.iter().all(|key| key.z == ZOOM));
    }
}
