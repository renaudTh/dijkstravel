//! Caméra inclinée : passage du monde à l'écran en perspective, à la manière de MapLibre. Repère local
//! centré sur le point visé : x vers l'est, y vers le nord, z vers le haut, en pixels monde au zoom de
//! la caméra ; l'altitude y est convertie à l'échelle du sol, exagérée.
//!
//! À inclinaison et cap nuls, un point à l'altitude du point visé tombe exactement là où le met `View` :
//! la distance de la caméra est choisie pour qu'un pixel monde y fasse un pixel d'écran.

use glam::dcamera::rh::{proj, view};
use glam::{DMat4, DVec2, DVec3, DVec4};

use crate::frame::Camera;
use crate::geo::LonLat;
use crate::mercator::{Projection, project, unproject};

/// Champ de vision vertical de MapLibre (`0.6435011087932844` rad, soit 36,87°).
const FOV_Y: f64 = 0.643_501_108_793_284_4;
/// Au-delà de cet angle depuis la verticale, le sol n'est plus dessiné : brouillard, puis ciel.
const MAX_RAY_ANGLE: f64 = 78.0 * std::f64::consts::PI / 180.0;
/// Part de la distance dessinée où commence le brouillard, quand l'horizon est en vue.
const FOG_START: f64 = 0.45;
/// Le brouillard monte quand le haut de l'écran approche de `MAX_RAY_ANGLE` sur cet angle : à plat, aucun.
const FOG_RAMP: f64 = 25.0 * std::f64::consts::PI / 180.0;
/// Plan proche, en part de la distance de la caméra au point visé.
const NEAR: f64 = 0.01;
const EARTH_CIRCUMFERENCE: f64 = 40_075_016.686;
/// Points cherchés sur chaque bord de l'écran pour l'emprise au sol.
const EDGE_SAMPLES: usize = 8;
/// Hauteur du relief au-dessus du point visé que l'emprise prend en compte, en mètres : un sommet proche
/// se voit en bas de l'écran avant que son pied n'y entre.
const RELIEF_ABOVE: f64 = 2500.0;

pub struct Perspective {
    camera: Camera,
    width: f64,
    height: f64,
    /// Pixel monde, au zoom de la caméra, du point visé.
    target: [f64; 2],
    /// Altitude du point visé, en mètres : le sol à cette altitude est à z = 0.
    ground: f64,
    /// Pixels monde par mètre d'altitude, exagération comprise.
    z_per_meter: f64,
    /// Distance de la caméra au point visé, en pixels monde.
    distance: f64,
    /// Plus grande distance au sol plat (à l'altitude du point visé) vue depuis l'aplomb de la caméra, en
    /// pixels monde.
    max_ground: f64,
    /// `fog`.
    fog: [f64; 2],
    near: f64,
    far: f64,
    eye: DVec3,
    view_projection: DMat4,
    inverse: DMat4,
}

impl Perspective {
    /// `ground` : altitude du point visé (centre de la caméra), en mètres.
    pub fn new(camera: Camera, width: u32, height: u32, ground: f64, exaggeration: f64) -> Self {
        let (width, height) = (f64::from(width), f64::from(height));
        let target = project(camera.center, camera.zoom);
        let meters_per_pixel =
            EARTH_CIRCUMFERENCE * camera.center[1].to_radians().cos() / crate::mercator::world_size(camera.zoom);
        let distance = height / 2.0 / (FOV_Y / 2.0).tan();
        let (pitch, bearing) = (camera.pitch.to_radians(), camera.bearing.to_radians());
        let forward = DVec3::new(bearing.sin(), bearing.cos(), 0.0);
        let eye = (DVec3::Z * pitch.cos() - forward * pitch.sin()) * distance;
        // Le plus loin que voit un coin du haut de l'écran, borné par `MAX_RAY_ANGLE`.
        let half_diagonal = ((FOV_Y / 2.0).tan() * (1.0 + (width / height).powi(2)).sqrt()).atan();
        let top_ray = (pitch + half_diagonal).min(MAX_RAY_ANGLE);
        let max_ground = eye.z * top_ray.tan();
        let fog = fog_range(pitch, max_ground);
        // Jusqu'au bout du brouillard : un relief plus haut que le point visé se voit au-delà du sol plat.
        // Jusqu'au niveau de la mer : une vallée plus basse que le point visé se voit au-delà.
        let z_per_meter = exaggeration / meters_per_pixel;
        let far = (fog[1].hypot(eye.z + ground.max(0.0) * z_per_meter) + distance) * 1.01;
        let view = view::look_at_mat4(eye, DVec3::ZERO, forward * pitch.cos() + DVec3::Z * pitch.sin());
        let projection = proj::directx::perspective(FOV_Y, width / height, distance * NEAR, far);
        let view_projection = projection * view;
        Perspective {
            camera,
            width,
            height,
            target,
            ground,
            z_per_meter,
            distance,
            max_ground,
            fog,
            near: distance * NEAR,
            far,
            eye,
            view_projection,
            inverse: view_projection.inverse(),
        }
    }

    /// Repère local d'un point à `altitude` mètres.
    pub fn local(&self, point: LonLat, altitude: f64) -> DVec3 {
        let [x, y] = project(point, self.camera.zoom);
        DVec3::new(x - self.target[0], self.target[1] - y, (altitude - self.ground) * self.z_per_meter)
    }

    /// Le point à `altitude` mètres à l'écran, avec sa profondeur (0 au plan proche, 1 au plan lointain) ;
    /// `None` derrière la caméra ou hors du champ en profondeur.
    pub fn screen_at(&self, point: LonLat, altitude: f64) -> Option<([f64; 2], f64)> {
        let clip = self.view_projection * self.local(point, altitude).extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        if !(0.0..=1.0).contains(&ndc.z) {
            return None;
        }
        Some(([(ndc.x + 1.0) / 2.0 * self.width, (1.0 - ndc.y) / 2.0 * self.height], ndc.z))
    }

    /// Le point du sol (plan à l'altitude du point visé) sous le pixel `[x, y]`, s'il y en a un à moins
    /// de la distance dessinée.
    pub fn ground_at(&self, [x, y]: [f64; 2]) -> Option<LonLat> {
        let hit = self.ground_hit([x, y])?;
        if (hit.truncate() - self.eye.truncate()).length() > self.max_ground * 1.001 {
            return None;
        }
        Some(self.to_lon_lat(hit))
    }

    /// Emprise au sol de l'image, polygone convexe : le contour de l'écran projeté sur le plus bas et le plus
    /// haut relief possibles (le niveau de la mer, `RELIEF_ABOVE` au-dessus du point visé), coupé au bout du
    /// brouillard. Plus bas que le point visé, une vallée se voit au-delà du sol plat ; plus haut, un
    /// sommet se voit en deçà.
    /// Sert à choisir les morceaux d'atlas et les tuiles de relief.
    pub fn ground_footprint(&self) -> Vec<LonLat> {
        let low = -self.ground.max(0.0) * self.z_per_meter;
        let high = (RELIEF_ABOVE * self.z_per_meter).min(self.eye.z * 0.5);
        let points: Vec<DVec2> = [low, high].into_iter().flat_map(|z| self.outline_at(z)).collect();
        convex_hull(points).into_iter().map(|p| self.to_lon_lat(p.extend(0.0))).collect()
    }

    /// Le contour de l'écran projeté sur le plan horizontal `z`, coupé au bout du brouillard.
    fn outline_at(&self, z: f64) -> Vec<DVec2> {
        let (w, h) = (self.width, self.height);
        let n = EDGE_SAMPLES as f64;
        let border = (0..EDGE_SAMPLES)
            .map(|i| [w * i as f64 / n, 0.0])
            .chain((0..EDGE_SAMPLES).map(|i| [w, h * i as f64 / n]))
            .chain((0..EDGE_SAMPLES).map(|i| [w - w * i as f64 / n, h]))
            .chain((0..EDGE_SAMPLES).map(|i| [0.0, h - h * i as f64 / n]));
        let eye = self.eye.truncate();
        border
            .map(|pixel| {
                let direction = self.ray(pixel);
                let horizontal = direction.truncate();
                // Le rayon touche le plan devant la caméra et assez près : ce point ; sinon, le plus loin
                // dessiné dans sa direction (le bord du brouillard).
                let reach = self.fog[1];
                let hit = (direction.z < -1e-9)
                    .then(|| (self.eye + direction * ((z - self.eye.z) / direction.z)).truncate())
                    .filter(|hit| (*hit - eye).length() <= reach);
                hit.unwrap_or_else(|| {
                    let along = if horizontal.length() > 1e-12 { horizontal.normalize() } else { horizontal };
                    eye + along * reach
                })
            })
            .collect()
    }

    /// Profondeur `screen_at` (non linéaire) changée en distance à la caméra le long de l'axe de visée,
    /// en pixels monde : deux profondeurs s'y comparent avec une marge relative.
    pub fn linear_depth(&self, depth: f64) -> f64 {
        self.near * self.far / (self.far - depth * (self.far - self.near))
    }

    /// Caméra en repère local.
    pub fn eye(&self) -> DVec3 {
        self.eye
    }

    /// Plus grande distance au sol dessinée depuis l'aplomb de la caméra, en pixels monde : la fin du
    /// brouillard.
    pub fn max_ground(&self) -> f64 {
        self.max_ground
    }

    /// Distances au sol (pixels monde) où le brouillard commence et où il recouvre tout : rien à plat,
    /// une brume vers l'horizon quand la caméra le voit.
    pub fn fog(&self) -> [f64; 2] {
        self.fog
    }

    /// Pixel monde, au zoom de la caméra, du point visé.
    pub fn target(&self) -> [f64; 2] {
        self.target
    }

    pub fn ground(&self) -> f64 {
        self.ground
    }

    pub fn z_per_meter(&self) -> f64 {
        self.z_per_meter
    }

    pub fn size(&self) -> [f64; 2] {
        [self.width, self.height]
    }

    /// L'horizon (le sol au bout du brouillard, droit devant) en y normalisé : −1 en bas de l'écran, 1 en
    /// haut ; plus de 1 s'il est au-dessus de l'écran.
    pub fn horizon(&self) -> f64 {
        let bearing = self.camera.bearing.to_radians();
        let ahead = self.eye.truncate() + glam::DVec2::new(bearing.sin(), bearing.cos()) * self.max_ground;
        let clip = self.view_projection * ahead.extend(0.0).extend(1.0);
        if clip.w <= 0.0 { 2.0 } else { clip.y / clip.w }
    }

    /// Distance de la caméra au point visé, en pixels monde : l'échelle du brouillard.
    pub fn distance(&self) -> f64 {
        self.distance
    }

    /// Matrice monde local → écran, pour le GPU (repère local centré sur le point visé).
    pub fn inverse_view_projection(&self) -> DMat4 {
        self.inverse
    }

    pub fn view_projection(&self) -> DMat4 {
        self.view_projection
    }

    pub fn camera(&self) -> Camera {
        self.camera
    }

    /// Direction, en repère local, du rayon qui part de la caméra par le pixel.
    fn ray(&self, [x, y]: [f64; 2]) -> DVec3 {
        let ndc = [x / self.width * 2.0 - 1.0, 1.0 - y / self.height * 2.0];
        let unproject = |z: f64| {
            let p: DVec4 = self.inverse * DVec4::new(ndc[0], ndc[1], z, 1.0);
            p.truncate() / p.w
        };
        (unproject(1.0) - unproject(0.0)).normalize()
    }

    /// Point du plan z = 0 sous le pixel, si le rayon y descend.
    pub(crate) fn ground_hit(&self, pixel: [f64; 2]) -> Option<DVec3> {
        let direction = self.ray(pixel);
        // Rayon horizontal ou montant : il ne touche jamais le sol.
        if direction.z >= -1e-9 {
            return None;
        }
        let t = -self.eye.z / direction.z;
        Some(self.eye + direction * t)
    }

    pub(crate) fn to_lon_lat(&self, local: DVec3) -> LonLat {
        unproject([self.target[0] + local.x, self.target[1] - local.y], self.camera.zoom)
    }
}

impl Projection for Perspective {
    /// Le point posé au sol à l'altitude du point visé.
    fn screen_point(&self, point: LonLat) -> Option<[f64; 2]> {
        self.screen_at(point, self.ground).map(|(screen, _)| screen)
    }

    fn camera_zoom(&self) -> f64 {
        self.camera.zoom
    }
}

/// Enveloppe convexe, dans le sens trigonométrique (chaîne monotone d'Andrew).
fn convex_hull(mut points: Vec<DVec2>) -> Vec<DVec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let turn = |o: DVec2, a: DVec2, b: DVec2| (a - o).perp_dot(b - o);
    let mut hull: Vec<DVec2> = Vec::with_capacity(points.len() * 2);
    let reversed: Vec<DVec2> = points.iter().rev().copied().collect();
    for pass in [&points, &reversed] {
        let start = hull.len();
        for &p in pass {
            while hull.len() >= start + 2 && turn(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

/// Distances au sol (pixels monde, depuis l'aplomb de la caméra) où le brouillard commence et où il
/// recouvre tout : rien à plat, une brume vers l'horizon quand la caméra le voit.
fn fog_range(pitch: f64, max_ground: f64) -> [f64; 2] {
    let reach = pitch + FOV_Y / 2.0;
    let t = ((reach - (MAX_RAY_ANGLE - FOG_RAMP)) / FOG_RAMP).clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    let lerp = |a: f64, b: f64| a + (b - a) * t;
    [max_ground * lerp(1.2, FOG_START), max_ground * lerp(1.5, 1.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(pitch: f64, bearing: f64) -> Camera {
        Camera { pitch, bearing, ..Camera::flat([6.06, 45.06], 13.4) }
    }

    #[test]
    fn flat_and_north_up_it_matches_the_flat_view() {
        let perspective = Perspective::new(camera(0.0, 0.0), 1080, 1920, 1500.0, 1.5);
        // `View` sans son calage au pixel entier.
        let center = project([6.06, 45.06], 13.4);
        for point in [[6.06, 45.06], [6.08, 45.05], [6.03, 45.09], [6.1, 45.0]] {
            let [x, y] = perspective.screen_point(point).unwrap();
            let [px, py] = project(point, 13.4);
            let (vx, vy) = (px - center[0] + 540.0, py - center[1] + 960.0);
            assert!((x - vx).abs() < 1e-6 && (y - vy).abs() < 1e-6, "{point:?} : {x} {y} ≠ {vx} {vy}");
        }
    }

    #[test]
    fn the_target_stays_at_the_centre_and_the_ground_round_trips() {
        let perspective = Perspective::new(camera(55.0, 120.0), 1080, 1920, 800.0, 1.0);
        let [x, y] = perspective.screen_point([6.06, 45.06]).unwrap();
        assert!((x - 540.0).abs() < 1e-6 && (y - 960.0).abs() < 1e-6);
        for pixel in [[100.0, 1800.0], [540.0, 1200.0], [900.0, 700.0]] {
            let point = perspective.ground_at(pixel).unwrap();
            let [x, y] = perspective.screen_point(point).unwrap();
            assert!((x - pixel[0]).abs() < 1e-4 && (y - pixel[1]).abs() < 1e-4, "{pixel:?} → {x} {y}");
        }
    }

    #[test]
    fn the_way_ahead_is_up_the_screen_and_what_is_behind_is_hidden() {
        // Cap à l'est : un point à l'est du centre monte à l'écran, un point à l'ouest descend.
        let perspective = Perspective::new(camera(60.0, 90.0), 1080, 1920, 0.0, 1.0);
        let ahead = perspective.screen_point([6.08, 45.06]).unwrap();
        let behind = perspective.screen_point([6.055, 45.06]).unwrap();
        assert!(ahead[1] < 960.0 && behind[1] > 960.0);
        // Loin derrière la caméra : rien.
        assert!(perspective.screen_point([5.5, 45.06]).is_none());
        // Un sommet plus haut que le sol monte à l'écran.
        let peak = perspective.screen_at([6.08, 45.06], 1000.0).unwrap().0;
        assert!(peak[1] < ahead[1]);
    }

    #[test]
    fn the_footprint_widens_ahead_and_reaches_the_fog_when_tilted_up() {
        let flat = Perspective::new(camera(0.0, 0.0), 1080, 1920, 0.0, 1.0);
        let tilted = Perspective::new(camera(70.0, 0.0), 1080, 1920, 0.0, 1.0);
        let north = |footprint: Vec<LonLat>| footprint.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        assert!(north(tilted.ground_footprint()) > north(flat.ground_footprint()));
        // À 70°, le haut de l'écran voit le ciel : pas de sol sous ce pixel.
        assert!(tilted.ground_at([540.0, 0.0]).is_none());
        // Vue à plat d'un point à 1000 m : le relief plus bas et plus haut élargit l'emprise.
        let high = Perspective::new(camera(0.0, 0.0), 1080, 1920, 1000.0, 1.0);
        let span = |footprint: Vec<LonLat>| {
            let lats = footprint.iter().map(|p| p[1]);
            lats.clone().fold(f64::MIN, f64::max) - lats.fold(f64::MAX, f64::min)
        };
        assert!(span(high.ground_footprint()) > span(flat.ground_footprint()));
    }
}
