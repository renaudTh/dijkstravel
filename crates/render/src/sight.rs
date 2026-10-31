//! Inclinaison qui garde la tête en vue dans la vue en relief : à chaque image du parcours, la plus forte,
//! jusqu'à celle du thème, pour laquelle le relief ne coupe ni la visée de la caméra vers la tête ni le
//! premier plan. Dans une vallée encaissée, la caméra se redresse plutôt que de regarder une crête.
//!
//! L'altitude du point visé, qui porte la caméra, est la moyenne du relief autour de lui (`ground`) :
//! la caméra ne saute pas d'une crête à une vallée quand le centre de l'image passe de l'une à l'autre.

use std::collections::BTreeSet;

use glam::DVec3;
use rayon::prelude::*;

use crate::elevation::Elevation;
use crate::frame::{Animation, Camera};
use crate::geo::LonLat;
use crate::mercator::{project, unproject};
use crate::perspective::Perspective;

/// Zoom du relief lu pour les visées (≈ 55 m par pixel dans les Alpes) : les crêtes y sont toutes.
pub const ZOOM: u8 = 11;
/// Inclinaisons essayées, de celle du thème vers la verticale, par pas de tant de degrés.
const STEP_DEGREES: f64 = 5.0;
/// Points de relief lus le long d'une visée.
const SAMPLES: usize = 48;
/// Part de la visée vers la tête, depuis la caméra, que le relief ne doit pas couper : le dernier bout
/// longe le sol où elle est posée.
const HEAD_CLEARANCE: f64 = 0.95;
/// La tête est visée un peu au-dessus du sol, en mètres.
const HEAD_LIFT: f64 = 15.0;
/// Part de la visée du bas de l'écran, depuis la caméra, que le relief ne doit pas couper : un sommet au
/// ras de la caméra boucherait l'image.
const NEAR_CLEARANCE: f64 = 0.5;
/// Images du parcours dont on lit les tuiles : les visées bougent peu d'une image à l'autre.
const TILE_FRAME_STEP: usize = 4;
/// Points lus le long d'une visée pour ses tuiles.
const TILE_SAMPLES: usize = 16;
/// Rayon du relief moyenné sous le point visé, en part du petit côté de l'écran.
const GROUND_RADIUS_SCREENS: f64 = 0.3;
/// Points de la grille du relief moyenné, par côté.
const GROUND_GRID: usize = 9;

/// Altitude du point visé par `camera`, en mètres : le relief autour du centre, pesé en cône jusqu'à
/// `GROUND_RADIUS_SCREENS`.
pub fn ground(elevation: &Elevation, camera: Camera, size: [u32; 2]) -> f64 {
    let zoom = ZOOM.min(elevation.maxzoom);
    let (sum, weights) = ground_points(camera, size).into_iter().fold((0.0, 0.0), |(sum, weights), (point, weight)| {
        (sum + elevation.altitude_at(point, zoom) * weight, weights + weight)
    });
    sum / weights
}

/// Grille autour du centre de la caméra, avec le poids de chaque point. Au point visé, un pixel monde
/// fait un pixel d'écran.
fn ground_points(camera: Camera, size: [u32; 2]) -> Vec<(LonLat, f64)> {
    let [x, y] = project(camera.center, camera.zoom);
    let radius = f64::from(size[0].min(size[1])) * GROUND_RADIUS_SCREENS;
    let offset = |i: usize| (2 * i) as f64 / (GROUND_GRID - 1) as f64 - 1.0;
    let mut points = Vec::new();
    for i in 0..GROUND_GRID {
        for j in 0..GROUND_GRID {
            let (dx, dy) = (offset(i), offset(j));
            let weight = 1.0 - dx.hypot(dy);
            if weight > 0.0 {
                points.push((unproject([x + dx * radius, y + dy * radius], camera.zoom), weight));
            }
        }
    }
    points
}

/// Plus forte inclinaison dégagée à chaque image du parcours (`Animation::travel_meters`).
pub fn limits(animation: &Animation, elevation: &Elevation, size: [u32; 2], exaggeration: f64) -> Vec<f64> {
    let zoom = ZOOM.min(elevation.maxzoom);
    let altitude = |point: LonLat| elevation.altitude_at(point, zoom);
    let path = &animation.tour.path;
    let top = animation.pitch();
    animation
        .travel_meters()
        .par_iter()
        .map(|&meters| {
            let head = path.point_at(meters);
            let mut pitch = top;
            loop {
                let camera = animation.tilted_camera(meters, pitch);
                let view = Perspective::new(camera, size[0], size[1], ground(elevation, camera, size), exaggeration);
                if pitch <= 0.0 || clear(&view, head, &altitude) {
                    return pitch.max(0.0);
                }
                pitch -= STEP_DEGREES;
            }
        })
        .collect()
}

/// Tuiles de relief que lit `limits`, `<source>/<z>/<x>/<y>`. Aux inclinaisons plus faibles que celle du
/// thème, la caméra se rapproche de l'aplomb du point visé : ses visées restent dans celles du thème.
pub fn tiles(animation: &Animation, elevation: &Elevation, size: [u32; 2], exaggeration: f64) -> BTreeSet<String> {
    let zoom = ZOOM.min(elevation.maxzoom);
    let path = &animation.tour.path;
    let mut tiles = BTreeSet::new();
    for &meters in animation.travel_meters().iter().step_by(TILE_FRAME_STEP) {
        let camera = animation.tilted_camera(meters, animation.pitch());
        let view = Perspective::new(camera, size[0], size[1], 0.0, exaggeration);
        let eye = view.eye().with_z(0.0);
        let head = view.local(path.point_at(meters), 0.0);
        for (point, _) in ground_points(camera, size) {
            tiles.extend(elevation.tiles_at(point, zoom));
        }
        for end in [DVec3::ZERO, head] {
            for i in 0..=TILE_SAMPLES {
                let point = view.to_lon_lat(eye.lerp(end, i as f64 / TILE_SAMPLES as f64));
                tiles.extend(elevation.tiles_at(point, zoom));
            }
        }
    }
    elevation.tile_names(&tiles.into_iter().collect::<Vec<_>>()).into_iter().collect()
}

/// Ni la visée vers la tête ni celle du bas de l'écran ne passent sous le relief.
fn clear(view: &Perspective, head: LonLat, altitude: &dyn Fn(LonLat) -> f64) -> bool {
    let head = view.local(head, altitude(head) + HEAD_LIFT);
    let [width, height] = view.size();
    let near = view.ground_hit([width / 2.0, height]);
    unblocked(view, head, HEAD_CLEARANCE, altitude)
        && near.is_none_or(|near| unblocked(view, near, NEAR_CLEARANCE, altitude))
}

/// La visée de la caméra vers `end` (repère local) passe au-dessus du relief sur sa part `share`.
fn unblocked(view: &Perspective, end: DVec3, share: f64, altitude: &dyn Fn(LonLat) -> f64) -> bool {
    let eye = view.eye();
    (1..=SAMPLES).all(|i| {
        let point = eye.lerp(end, share * i as f64 / SAMPLES as f64);
        let relief = (altitude(view.to_lon_lat(point)) - view.ground()) * view.z_per_meter();
        relief <= point.z
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(pitch: f64) -> Perspective {
        let camera = Camera { pitch, bearing: 0.0, ..Camera::flat([6.0, 45.0], 13.0) };
        Perspective::new(camera, 1080, 1920, 1000.0, 1.5)
    }

    #[test]
    fn a_ridge_between_the_camera_and_the_head_blocks_it() {
        let head = [6.0, 45.0];
        let flat = |_: LonLat| 1000.0;
        assert!(clear(&view(55.0), head, &flat));
        // Une crête de 1500 m au sud, entre la caméra (qui regarde vers le nord) et la tête.
        let ridge = |[_, lat]: LonLat| if (45.0 - lat) > 0.002 && (45.0 - lat) < 0.02 { 2500.0 } else { 1000.0 };
        assert!(!clear(&view(55.0), head, &ridge));
        // À la verticale, la caméra voit par-dessus.
        assert!(clear(&view(0.0), head, &ridge));
    }
}
