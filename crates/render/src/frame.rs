//! Phases, caméra et carte d'infos de chaque image : elles ne dépendent que de son numéro.

use std::sync::Arc;

use crate::geo::LonLat;
use crate::model::{Tour, Video};

/// En nombre d'images.
#[derive(Debug, Clone, Copy)]
pub struct Timeline {
    pub intro: u32,
    pub travel: u32,
    pub outro: u32,
}

impl Timeline {
    pub fn new(video: &Video) -> Self {
        let fps = f64::from(video.fps);
        Timeline {
            intro: (video.intro_sec * fps).round() as u32,
            travel: ((video.duration_sec * fps).round() as u32).max(1),
            outro: (video.outro_sec * fps).round() as u32,
        }
    }

    pub fn frame_count(&self) -> u32 {
        self.intro + self.travel + self.outro
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Moment {
    Intro { progress: f64 },
    Travel { meters: f64 },
    Outro { progress: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hud {
    pub meters: f64,
    pub total_meters: f64,
    /// Les `visited` premiers lieux de `Tour::stops` sont visités.
    pub visited: usize,
    pub total: usize,
    pub last_stop: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub center: LonLat,
    pub zoom: f64,
}

const SMOOTHING_SAMPLES: usize = 21;
// ponytail: zoom calculé pour un écran de 1080 px de petit côté, vrai pour les deux formats vidéo.
const SCREEN_PIXELS: f64 = 1080.0;
const WORLD_METERS: f64 = 40_075_016.0;
const PACE_SAMPLES: usize = 8192;
/// Demi-largeur de la zone ralentie autour d'un lieu, en part de l'écart moyen entre deux lieux.
const SLOW_ZONE: f64 = 0.25;

fn progress_of(index: u32, count: u32) -> f64 {
    if count <= 1 { 1.0 } else { (f64::from(index) / f64::from(count - 1)).min(1.0) }
}

/// Distances échantillonnées le long du trajet et temps relatif cumulé pour atteindre chacune.
struct Pace {
    meters: Vec<f64>,
    time: Vec<f64>,
}

/// Tout ce qui ne dépend que de la tournée et des réglages, calculé une fois.
pub struct Animation {
    pub tour: Arc<Tour>,
    pub timeline: Timeline,
    follow_fraction: f64,
    slowdown: f64,
    total: f64,
    pace: Option<Pace>,
    overview: Camera,
}

impl Animation {
    pub fn new(tour: Arc<Tour>, video: &Video, follow_fraction: f64, slowdown: f64) -> Self {
        let total = tour.path.total_meters();
        let pace = (slowdown != 0.0 && total != 0.0).then(|| pace_of(&tour, total, slowdown));
        let overview = overview(&tour);
        Animation { tour, timeline: Timeline::new(video), follow_fraction, slowdown, total, pace, overview }
    }

    pub fn total_meters(&self) -> f64 {
        self.total
    }

    pub fn moment_at(&self, frame: u32) -> Moment {
        let timeline = self.timeline;
        if frame < timeline.intro {
            return Moment::Intro { progress: progress_of(frame, timeline.intro) };
        }
        let travel_frame = frame - timeline.intro;
        if travel_frame < timeline.travel {
            return Moment::Travel { meters: self.meters_at(progress_of(travel_frame, timeline.travel)) };
        }
        Moment::Outro { progress: progress_of(travel_frame - timeline.travel, timeline.outro) }
    }

    /// Mètres parcourus à l'image, quelle que soit la phase.
    pub fn meters_of(&self, moment: Moment) -> f64 {
        match moment {
            Moment::Intro { .. } => 0.0,
            Moment::Travel { meters } => meters,
            Moment::Outro { .. } => self.total,
        }
    }

    fn meters_at(&self, progress: f64) -> f64 {
        let Some(Pace { meters, time }) = self.pace.as_ref().filter(|_| progress < 1.0) else {
            return self.total * progress;
        };
        debug_assert!(self.slowdown != 0.0);
        let target = progress * time.last().copied().unwrap_or(0.0);
        let (mut low, mut high) = (0, time.len() - 1);
        while high - low > 1 {
            let middle = (low + high) >> 1;
            if time[middle] <= target {
                low = middle;
            } else {
                high = middle;
            }
        }
        let (t0, t1, m0, m1) = (time[low], time[high], meters[low], meters[high]);
        if t1 == t0 { m0 } else { lerp(m0, m1, (target - t0) / (t1 - t0)) }
    }

    /// Un lieu est visité dès que `meters` atteint sa distance d'arrivée.
    pub fn hud_at(&self, meters: f64) -> Hud {
        let clamped = meters.max(0.0).min(self.total);
        let stops = &self.tour.stops;
        let visited = stops.iter().filter(|stop| stop.meters <= clamped).count();
        Hud {
            meters: clamped,
            total_meters: self.total,
            visited,
            total: stops.len(),
            last_stop: visited.checked_sub(1).map(|i| stops[i].name.clone()),
        }
    }

    pub fn camera_at(&self, moment: Moment) -> Camera {
        match moment {
            Moment::Intro { progress } => blend(self.overview, self.travel_camera(0.0), smoothstep(progress)),
            Moment::Travel { meters } => self.travel_camera(meters),
            Moment::Outro { progress } => blend(self.travel_camera(self.total), self.overview, smoothstep(progress)),
        }
    }

    /// Zoom de tout le parcours : il ne dépend pas de la position.
    pub fn travel_zoom(&self) -> f64 {
        (self.overview.zoom + (1.0 / self.follow_fraction).log2()).clamp(4.0, 16.0)
    }

    /// Nord en haut, à plat ; le centre suit une moyenne glissante du tracé, pas la tête elle-même.
    fn travel_camera(&self, meters: f64) -> Camera {
        let zoom = self.travel_zoom();
        let visible_meters =
            (WORLD_METERS * self.overview.center[1].to_radians().cos() * SCREEN_PIXELS) / (512.0 * 2f64.powf(zoom));
        let window = visible_meters * 0.5;
        let mut sum = [0.0, 0.0];
        for i in 0..SMOOTHING_SAMPLES {
            let offset = (2 * i) as f64 / (SMOOTHING_SAMPLES - 1) as f64 - 1.0;
            let point = self.tour.path.point_at(meters + window * offset);
            sum = [sum[0] + point[0], sum[1] + point[1]];
        }
        let count = SMOOTHING_SAMPLES as f64;
        Camera { center: [sum[0] / count, sum[1] / count], zoom }
    }
}

fn pace_of(tour: &Tour, total: f64, slowdown: f64) -> Pace {
    let radius = (SLOW_ZONE * total) / (tour.stops.len().max(1) as f64);
    let meters: Vec<f64> = (0..=PACE_SAMPLES).map(|i| (total * i as f64) / PACE_SAMPLES as f64).collect();
    // Le plus fort ralentissement, pas leur somme : un groupe de lieux proches ne s'éternise pas.
    let slowness: Vec<f64> = meters
        .iter()
        .map(|&at| {
            let strongest = tour.stops.iter().map(|stop| bump((at - stop.meters).abs() / radius)).fold(0.0, f64::max);
            1.0 + slowdown * strongest
        })
        .collect();
    let mut time = vec![0.0];
    for i in 1..meters.len() {
        let step = ((slowness[i - 1] + slowness[i]) / 2.0) * (meters[i] - meters[i - 1]);
        time.push(time[i - 1] + step);
    }
    Pace { meters, time }
}

/// 1 sur le lieu, 0 à partir d'une demi-zone, en cloche : la caméra freine et repart sans à-coup.
fn bump(distance: f64) -> f64 {
    if distance >= 1.0 { 0.0 } else { (1.0 + (std::f64::consts::PI * distance).cos()) / 2.0 }
}

fn overview(tour: &Tour) -> Camera {
    fit(tour.path.points.iter().copied())
}

/// La caméra qui cadre tous les `points` (80 % de l'écran), nord en haut.
fn fit(points: impl IntoIterator<Item = LonLat>) -> Camera {
    let (mut west, mut east, mut south, mut north) =
        (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for [lon, lat] in points {
        let y = mercator_degrees(lat);
        (west, east, south, north) = (west.min(lon), east.max(lon), south.min(y), north.max(y));
    }
    let span = (east - west).max(north - south).max(0.01);
    Camera {
        center: [(west + east) / 2.0, inverse_mercator_degrees((south + north) / 2.0)],
        zoom: ((SCREEN_PIXELS * 0.8 * 360.0) / (512.0 * span)).log2().clamp(1.0, 16.0),
    }
}

fn blend(from: Camera, to: Camera, t: f64) -> Camera {
    Camera {
        center: [lerp(from.center[0], to.center[0], t), lerp(from.center[1], to.center[1], t)],
        zoom: lerp(from.zoom, to.zoom, t),
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn smoothstep(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn mercator_degrees(lat: f64) -> f64 {
    (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0).tan().ln().to_degrees()
}

fn inverse_mercator_degrees(y: f64) -> f64 {
    (2.0 * y.to_radians().exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees()
}
