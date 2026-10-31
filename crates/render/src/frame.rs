//! Phases, caméra et carte d'infos de chaque image : elles ne dépendent que de son numéro.

use std::sync::{Arc, OnceLock};

use crate::geo::{LonLat, MeasuredPath};
use crate::mercator::project;
use crate::model::{CameraTheme, Tour, Video};

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
    /// Inclinaison en degrés : 0, vue du dessus.
    pub pitch: f64,
    /// Cap en haut de l'écran, en degrés dans le sens horaire depuis le nord.
    pub bearing: f64,
}

impl Camera {
    /// Vue du dessus, nord en haut.
    pub fn flat(center: LonLat, zoom: f64) -> Self {
        Camera { center, zoom, pitch: 0.0, bearing: 0.0 }
    }
}

const SMOOTHING_SAMPLES: usize = 21;
// ponytail: zoom calculé pour un écran de 1080 px de petit côté, vrai pour les deux formats vidéo.
const SCREEN_PIXELS: f64 = 1080.0;
const WORLD_METERS: f64 = 40_075_016.0;
const PACE_SAMPLES: usize = 8192;
/// Demi-largeur de la zone ralentie autour d'un lieu, en part de l'écart moyen entre deux lieux.
const SLOW_ZONE: f64 = 0.25;
/// Un transfert entre deux étapes dure au plus le temps de parcourir cette part d'écran en course.
const TRANSFER_SCREENS: f64 = 0.5;
/// Part de chaque bout d'un transfert sur laquelle se fait le fondu vers son fond, et retour.
const TRANSFER_FADE: f64 = 0.25;
/// Demi-zone de ralentissement autour d'un repère, en écrans (petit côté) : la caméra freine en l'approchant.
const MARKER_SCREENS: f64 = 0.6;
/// Échantillons du cap par fenêtre de lissage (`camera.headingWindow`).
const HEADING_SAMPLES_PER_WINDOW: f64 = 32.0;
/// La corde qui donne le cap brut commence cette part de fenêtre derrière la tête : la caméra regarde
/// surtout où va le tracé.
const HEADING_BEHIND: f64 = 0.5;
/// Netteté du tracé (corde sur longueur parcourue) sous laquelle la caméra garde son cap, et au-dessus de
/// laquelle elle suit pleinement le tracé : une boucle, un circuit à plusieurs tours, des lacets serrés
/// ne la font pas tourner.
const STRAIGHTNESS: [f64; 2] = [0.3, 0.6];
/// Rotation la plus rapide de la caméra, en degrés par seconde de vidéo.
const MAX_TURN_PER_SECOND: f64 = 20.0;
/// L'inclinaison change d'au plus tant de degrés par seconde quand le relief l'oblige à se redresser.
const MAX_PITCH_PER_SECOND: f64 = 6.0;
/// Le plafond d'inclinaison d'une image vaut pour tant de secondes autour d'elle : la caméra se redresse
/// avant que le relief ne cache la tête et reste redressée sans à-coups entre deux obstacles proches.
const PITCH_HOLD_SECONDS: f64 = 1.5;

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
    /// `camera.zoom` : resserre le cadrage, pas le lissage.
    zoom: f64,
    total: f64,
    pace: Option<Pace>,
    overview: Camera,
    /// Un transfert dure au plus le temps de parcourir cette distance en course.
    transfer_meters: f64,
    /// Inclinaison du parcours (`camera.pitch`).
    pitch: f64,
    /// Cap de la caméra inclinée le long du tracé ; absent à plat.
    headings: Option<Headings>,
    /// Inclinaison de chaque image du parcours, redressée là où le relief cacherait la tête
    /// (`hold_pitches`) ; sans elle, celle du thème partout.
    pitches: OnceLock<PerFrame>,
    fps: f64,
}

impl Animation {
    pub fn new(tour: Arc<Tour>, video: &Video, camera: &CameraTheme) -> Self {
        let follow_fraction = camera.follow_fraction;
        let total = tour.path.total_meters();
        let overview = overview(&tour);
        // Lissage et zones à l'échelle de l'écran sans `camera.zoom` : zoomer ne rend pas la caméra plus nerveuse.
        let screen = visible_meters(overview, travel_zoom(overview, follow_fraction));
        let zones = Zones { transfer: TRANSFER_SCREENS * screen, marker: MARKER_SCREENS * screen };
        let paced = camera.stop_slowdown != 0.0
            || !tour.transfers.is_empty()
            || (camera.even_stages != 0.0 && !tour.stages.is_empty())
            || (camera.marker_slowdown != 0.0 && !tour.markers.is_empty());
        let pace = (paced && total != 0.0).then(|| pace_of(&tour, total, camera, zones));
        let transfer_meters = zones.transfer;
        let mut animation = Animation {
            tour,
            timeline: Timeline::new(video),
            follow_fraction,
            zoom: camera.zoom.max(f64::MIN_POSITIVE),
            total,
            pace,
            overview,
            transfer_meters,
            pitch: camera.pitch,
            headings: None,
            pitches: OnceLock::new(),
            fps: f64::from(video.fps.max(1)),
        };
        if camera.pitch > 0.0 {
            let frames = animation.travel_meters();
            let max_turn = MAX_TURN_PER_SECOND / f64::from(video.fps.max(1));
            let course = Course::new(&animation.tour.path, camera.heading_window * screen);
            animation.headings = Some(Headings::new(&course, frames, max_turn));
        }
        animation
    }

    /// Part du fond des transferts à `meters` (`transfer_blend`).
    pub fn transfer_blend(&self, meters: f64) -> f64 {
        transfer_blend(meters, &self.tour.transfers, self.transfer_meters)
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
            meters: self.tour.raced_meters(clamped),
            total_meters: self.tour.raced_meters(self.total),
            visited,
            total: stops.len(),
            // Une course par étapes affiche l'étape en cours plutôt que le dernier lieu.
            last_stop: match self.tour.stage_at(clamped) {
                Some(stage) => Some(stage.name.clone()),
                None => visited.checked_sub(1).map(|i| stops[i].name.clone()),
            },
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
        (travel_zoom(self.overview, self.follow_fraction) + self.zoom.log2()).clamp(4.0, 16.0)
    }

    /// Mètres au sol sur le petit côté de l'écran sans `camera.zoom` : l'échelle du lissage de la caméra.
    fn smoothing_meters(&self) -> f64 {
        visible_meters(self.overview, travel_zoom(self.overview, self.follow_fraction))
    }

    /// Mètres au sol sur le petit côté de l'écran pendant le parcours.
    pub fn screen_meters(&self) -> f64 {
        visible_meters(self.overview, self.travel_zoom())
    }

    /// Inclinaison du parcours dans le thème, en degrés.
    pub fn pitch(&self) -> f64 {
        self.pitch
    }

    /// Distance parcourue à chaque image du parcours.
    pub fn travel_meters(&self) -> Vec<f64> {
        let travel = self.timeline.travel;
        (0..travel).map(|frame| self.meters_at(progress_of(frame, travel))).collect()
    }

    /// La caméra du parcours à `meters`, à l'inclinaison `pitch`.
    pub fn tilted_camera(&self, meters: f64, pitch: f64) -> Camera {
        Camera { pitch, ..self.travel_camera(meters) }
    }

    /// Plafonne l'inclinaison de chaque image du parcours (`limits`, une par image de `travel_meters`),
    /// une fois pour toutes : `limits` n'est appelé que la première fois. La caméra reste sous chaque plafond un peu
    /// avant et après son image, et ne tourne jamais plus vite que `MAX_PITCH_PER_SECOND`.
    pub fn hold_pitches(&self, limits: impl FnOnce() -> Vec<f64>) {
        self.pitches.get_or_init(|| {
            let limits = limits();
            let hold = (PITCH_HOLD_SECONDS * self.fps).round() as usize;
            let floor: Vec<f64> = (0..limits.len())
                .map(|i| {
                    limits[i.saturating_sub(hold)..(i + hold + 1).min(limits.len())]
                        .iter()
                        .copied()
                        .fold(self.pitch, f64::min)
                })
                .collect();
            let step = MAX_PITCH_PER_SECOND / self.fps;
            let mut held = floor.clone();
            for i in 1..held.len() {
                held[i] = held[i].min(held[i - 1] + step);
            }
            for i in (0..held.len().saturating_sub(1)).rev() {
                held[i] = held[i].min(held[i + 1] + step);
            }
            // Moyenne glissante : chaque valeur moyennée est sous le plancher de toutes les images à moins
            // de `hold`, donc sous le plafond de l'image.
            let values = (0..held.len())
                .map(|i| {
                    let window = &held[i.saturating_sub(hold)..(i + hold + 1).min(held.len())];
                    window.iter().sum::<f64>() / window.len() as f64
                })
                .collect();
            PerFrame { meters: self.travel_meters(), values }
        });
    }

    /// Le centre suit une moyenne glissante du tracé, pas la tête elle-même ; à plat nord en haut, incliné
    /// face au cap lissé du tracé.
    fn travel_camera(&self, meters: f64) -> Camera {
        let zoom = self.travel_zoom();
        let window = self.smoothing_meters() * 0.5;
        let mut sum = [0.0, 0.0];
        for i in 0..SMOOTHING_SAMPLES {
            let offset = (2 * i) as f64 / (SMOOTHING_SAMPLES - 1) as f64 - 1.0;
            let point = self.tour.path.point_at(meters + window * offset);
            sum = [sum[0] + point[0], sum[1] + point[1]];
        }
        let count = SMOOTHING_SAMPLES as f64;
        let center = [sum[0] / count, sum[1] / count];
        match &self.headings {
            Some(headings) => {
                let pitch = self.pitches.get().map_or(self.pitch, |pitches| pitches.at(meters));
                Camera { center, zoom, pitch, bearing: headings.at(meters) }
            }
            None => Camera::flat(center, zoom),
        }
    }
}

/// Cap du tracé le long du parcours, échantillonné tous les `step` mètres, déroulé (il peut dépasser 360).
struct Course {
    step: f64,
    bearings: Vec<f64>,
}

impl Course {
    /// `window` : longueur de tracé, en mètres, sur laquelle le cap est lissé.
    fn new(path: &MeasuredPath, window: f64) -> Self {
        let total = path.total_meters();
        let window = window.max(1.0);
        let step = window / HEADING_SAMPLES_PER_WINDOW;
        let count = (total / step).ceil() as usize + 1;
        // Cap brut : la corde du tracé autour de la tête, un peu en avant. Il ne compte qu'autant que le
        // tracé y est net : là où la corde est courte devant le chemin parcouru (boucle, circuit, lacets),
        // la caméra garde le cap qu'elle avait ; une corde nulle le garde entièrement.
        let mut held: Vec<f64> = Vec::with_capacity(count);
        for i in 0..count {
            let at = i as f64 * step;
            let (from, to) = ((at - window * HEADING_BEHIND).max(0.0), (at + window).min(total));
            let (a, b) = (path.point_at(from), path.point_at(to));
            let ([x0, y0], [x1, y1]) = (project(a, 0.0), project(b, 0.0));
            let (dx, dy) = (x1 - x0, y1 - y0);
            let latitude = ((a[1] + b[1]) / 2.0).to_radians();
            let chord = dx.hypot(dy) * WORLD_METERS * latitude.cos() / 512.0;
            let straightness = if to > from { chord / (to - from) } else { 0.0 };
            let [low, high] = STRAIGHTNESS;
            let follow = smoothstep(((straightness - low) / (high - low)).clamp(0.0, 1.0));
            // y Mercator vers le sud : le nord est en −y.
            let bearing = dx.atan2(-dy).to_degrees();
            held.push(match held.last() {
                Some(&previous) => previous + shortest_turn(previous, bearing) * follow,
                None => bearing,
            });
        }
        // Lissage gaussien d'une demi-fenêtre d'écart type.
        let sigma = HEADING_SAMPLES_PER_WINDOW / 2.0;
        let radius = (3.0 * sigma).ceil() as isize;
        let weights: Vec<f64> =
            (-radius..=radius).map(|k| (-(k as f64).powi(2) / (2.0 * sigma * sigma)).exp()).collect();
        let bearings = (0..count as isize)
            .map(|i| {
                let (mut sum, mut weight) = (0.0, 0.0);
                for (k, w) in (-radius..=radius).zip(&weights) {
                    let j = (i + k).clamp(0, count as isize - 1) as usize;
                    sum += held[j] * w;
                    weight += w;
                }
                sum / weight
            })
            .collect();
        Course { step, bearings }
    }

    fn at(&self, meters: f64) -> f64 {
        let last = self.bearings.len() - 1;
        let position = (meters / self.step).clamp(0.0, last as f64);
        let i = (position.floor() as usize).min(last.saturating_sub(1));
        match self.bearings.get(i + 1) {
            Some(&next) => lerp(self.bearings[i], next, position - i as f64),
            None => self.bearings[i],
        }
    }
}

/// Cap de la caméra inclinée à chaque image du parcours : celui du tracé, dont la rotation est bornée
/// d'une image à l'autre. Calculé une fois pour toutes les images, il se lit par la distance parcourue,
/// les images se rendant dans le désordre.
struct Headings {
    /// Degrés, dans le sens horaire depuis le nord, déroulés.
    bearings: PerFrame,
}

impl Headings {
    /// `max_turn` : degrés au plus d'une image à la suivante.
    fn new(course: &Course, meters: Vec<f64>, max_turn: f64) -> Self {
        let target: Vec<f64> = meters.iter().map(|&at| course.at(at)).collect();
        // Bornée dans un sens puis dans l'autre, la rotation en retard sur le tracé à l'aller, en avance
        // au retour : leur moyenne reste bornée et centrée.
        let clamp = |frames: &mut dyn Iterator<Item = &f64>| -> Vec<f64> {
            let mut out: Vec<f64> = Vec::with_capacity(target.len());
            for &bearing in frames {
                out.push(match out.last() {
                    Some(&previous) => previous + (bearing - previous).clamp(-max_turn, max_turn),
                    None => bearing,
                });
            }
            out
        };
        let forward = clamp(&mut target.iter());
        let mut backward = clamp(&mut target.iter().rev());
        backward.reverse();
        let values = forward.iter().zip(&backward).map(|(a, b)| (a + b) / 2.0).collect();
        Headings { bearings: PerFrame { meters, values } }
    }

    fn at(&self, meters: f64) -> f64 {
        self.bearings.at(meters).rem_euclid(360.0)
    }
}

/// Une valeur par image du parcours, lue par la distance parcourue : les images se rendent dans le
/// désordre.
struct PerFrame {
    /// Distance parcourue à chaque image du parcours, croissante.
    meters: Vec<f64>,
    values: Vec<f64>,
}

impl PerFrame {
    fn at(&self, meters: f64) -> f64 {
        let Some(&last) = self.values.last() else { return 0.0 };
        let i = self.meters.partition_point(|&m| m <= meters);
        match i {
            0 => self.values[0],
            i if i >= self.meters.len() => last,
            i => {
                let (m0, m1) = (self.meters[i - 1], self.meters[i]);
                let t = if m1 > m0 { (meters - m0) / (m1 - m0) } else { 0.0 };
                lerp(self.values[i - 1], self.values[i], t)
            }
        }
    }
}

/// Plus petit angle, signé, pour tourner du cap `from` au cap `to`, en degrés.
fn shortest_turn(from: f64, to: f64) -> f64 {
    (to - from + 180.0).rem_euclid(360.0) - 180.0
}

fn travel_zoom(overview: Camera, follow_fraction: f64) -> f64 {
    (overview.zoom + (1.0 / follow_fraction).log2()).clamp(4.0, 16.0)
}

/// Mètres au sol sur le petit côté de l'écran, au zoom `zoom`.
fn visible_meters(overview: Camera, zoom: f64) -> f64 {
    (WORLD_METERS * overview.center[1].to_radians().cos() * SCREEN_PIXELS) / (512.0 * 2f64.powf(zoom))
}

/// Longueurs de parcours à l'échelle de l'écran.
#[derive(Clone, Copy)]
struct Zones {
    /// Un transfert dure au plus le temps de parcourir cette distance en course.
    transfer: f64,
    /// Demi-zone de ralentissement autour d'un repère.
    marker: f64,
}

fn pace_of(tour: &Tour, total: f64, camera: &CameraTheme, zones: Zones) -> Pace {
    let radius = (SLOW_ZONE * total) / (tour.stops.len().max(1) as f64);
    let meters: Vec<f64> = (0..=PACE_SAMPLES).map(|i| (total * i as f64) / PACE_SAMPLES as f64).collect();
    // Le plus fort ralentissement, pas leur somme : un groupe de lieux proches ne s'éternise pas.
    let slowness: Vec<f64> = meters
        .iter()
        .map(|&at| {
            let near_stop = tour.stops.iter().map(|stop| bump((at - stop.meters).abs() / radius)).fold(0.0, f64::max);
            let near_marker =
                tour.markers.iter().map(|marker| bump((at - marker.meters).abs() / zones.marker)).fold(0.0, f64::max);
            let braking = (camera.stop_slowdown * near_stop).max(camera.marker_slowdown * near_marker);
            (1.0 + braking)
                * stage_slowness(tour, at, camera.even_stages)
                * transfer_slowness(at, &tour.transfers, zones.transfer)
        })
        .collect();
    let mut time = vec![0.0];
    for i in 1..meters.len() {
        let step = ((slowness[i - 1] + slowness[i]) / 2.0) * (meters[i] - meters[i - 1]);
        time.push(time[i - 1] + step);
    }
    Pace { meters, time }
}

/// Lenteur relative dans une étape de longueur `L` : `(L / moyenne)^-evenStages`, si bien qu'elle dure
/// comme `L^(1 - evenStages)`. Entre deux étapes, elle passe en douceur de l'une à l'autre.
fn stage_slowness(tour: &Tour, at: f64, even_stages: f64) -> f64 {
    let stages = &tour.stages;
    if even_stages == 0.0 || stages.is_empty() {
        return 1.0;
    }
    let mean = stages.iter().map(|stage| stage.to - stage.from).sum::<f64>() / stages.len() as f64;
    let slowness = |i: usize| {
        let length = (stages[i].to - stages[i].from).max(1.0);
        (length / mean).powf(-even_stages)
    };
    let next = stages.iter().position(|stage| at < stage.from);
    match next {
        Some(0) => slowness(0),
        None => slowness(stages.len() - 1),
        Some(i) if at <= stages[i - 1].to => slowness(i - 1),
        Some(i) => {
            let (from, to) = (stages[i - 1].to, stages[i].from);
            lerp(slowness(i - 1), slowness(i), smoothstep((at - from) / (to - from)))
        }
    }
}

/// Part du fond des transferts : elle monte au début du transfert et retombe à sa fin. Un transfert
/// plus court que `transfer_meters` ne l'atteint pas en entier : pas d'éclair de fond.
fn transfer_blend(meters: f64, transfers: &[[f64; 2]], transfer_meters: f64) -> f64 {
    let Some(&[from, to]) = transfers.iter().find(|&&[from, to]| from < meters && meters < to) else { return 0.0 };
    let length = to - from;
    let edge = (meters - from).min(to - meters) / length;
    (length / transfer_meters).min(1.0) * smoothstep((edge / TRANSFER_FADE).min(1.0))
}

/// Lenteur relative dans un transfert : la caméra accélère puis freine en cloche, jamais plus lente
/// qu'en course. Pour une pointe `s` fois plus rapide, le transfert prend `longueur / √s` : `s` est
/// choisi pour qu'il ne dépasse pas `transfer_meters`.
fn transfer_slowness(at: f64, transfers: &[[f64; 2]], transfer_meters: f64) -> f64 {
    let Some(&[from, to]) = transfers.iter().find(|&&[from, to]| from <= at && at <= to) else { return 1.0 };
    let length = to - from;
    if length <= transfer_meters {
        return 1.0;
    }
    let peak = (length / transfer_meters).powi(2);
    let wave = (std::f64::consts::PI * (at - from) / length).sin();
    1.0 / (1.0 + (peak - 1.0) * wave * wave)
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
    Camera::flat(
        [(west + east) / 2.0, inverse_mercator_degrees((south + north) / 2.0)],
        ((SCREEN_PIXELS * 0.8 * 360.0) / (512.0 * span)).log2().clamp(1.0, 16.0),
    )
}

/// Le cap tourne par le plus court chemin.
fn blend(from: Camera, to: Camera, t: f64) -> Camera {
    Camera {
        center: [lerp(from.center[0], to.center[0], t), lerp(from.center[1], to.center[1], t)],
        zoom: lerp(from.zoom, to.zoom, t),
        pitch: lerp(from.pitch, to.pitch, t),
        bearing: (from.bearing + shortest_turn(from.bearing, to.bearing) * t).rem_euclid(360.0),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_heading_faces_the_way_and_ignores_hairpins() {
        // Vers l'est.
        let east = MeasuredPath::measure(vec![[0.0, 45.0], [0.1, 45.0]]);
        let course = Course::new(&east, 1_000.0);
        assert!((course.at(4_000.0) - 90.0).abs() < 1e-6);
        // Des lacets de 200 m de large qui montent vers le nord : le cap lissé sur 2 km reste au nord.
        let zigzag: Vec<LonLat> =
            (0..=40).map(|i| [if i % 2 == 0 { 0.0 } else { 0.0025 }, 45.0 + i as f64 * 0.001]).collect();
        let course = Course::new(&MeasuredPath::measure(zigzag), 2_000.0);
        let bearing = course.at(2_500.0);
        assert!(shortest_turn(0.0, bearing).abs() < 15.0, "cap {bearing}");
    }

    #[test]
    fn laps_of_a_small_circuit_do_not_spin_the_camera() {
        // 10 km vers l'est, quatre tours d'un carré de 1 km, puis 10 km vers l'est.
        let (degree_lon, degree_lat) = (1.0 / 78.85, 1.0 / 111.2);
        let mut points: Vec<LonLat> = vec![[0.0, 45.0], [10.0 * degree_lon, 45.0]];
        for _ in 0..4 {
            for [dx, dy] in [[1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]] {
                points.push([(10.0 + dx) * degree_lon, 45.0 + dy * degree_lat]);
            }
        }
        points.push([20.0 * degree_lon, 45.0]);
        let path = MeasuredPath::measure(points);
        let course = Course::new(&path, 3_000.0);
        // Sur les quatre tours, le cap ne fait pas le tour : il reste vers l'est, à peine tourné vers le
        // circuit qui s'écarte au nord-est.
        let bearings: Vec<f64> = (10..=26).map(|km| course.at(f64::from(km) * 1_000.0)).collect();
        let (low, high) = bearings.iter().fold((f64::MAX, f64::MIN), |(l, h), &b| (l.min(b), h.max(b)));
        assert!(high - low < 60.0 && low > 30.0 && high < 120.0, "caps {bearings:?}");
        // Image par image, la rotation reste bornée, même au départ d'un tracé qui tourne d'un coup.
        let frames: Vec<f64> = (0..600).map(|i| path.total_meters() * i as f64 / 599.0).collect();
        let headings = Headings::new(&course, frames.clone(), 1.0);
        let turns = frames.windows(2).map(|m| shortest_turn(headings.at(m[0]), headings.at(m[1])).abs());
        assert!(turns.fold(0.0, f64::max) <= 1.0 + 1e-9);
    }

    #[test]
    fn the_bearing_blends_the_short_way_round() {
        let from = Camera { bearing: 350.0, ..Camera::flat([0.0, 0.0], 10.0) };
        let to = Camera { bearing: 10.0, pitch: 60.0, ..Camera::flat([0.0, 0.0], 10.0) };
        let half = blend(from, to, 0.5);
        assert!(half.bearing.abs() < 1e-9 || (half.bearing - 360.0).abs() < 1e-9);
        assert!((half.pitch - 30.0).abs() < 1e-9);
    }

    /// Temps d'un transfert, en mètres de course : l'intégrale de sa lenteur.
    fn transfer_time(length: f64, transfer_meters: f64) -> f64 {
        let steps = 100_000;
        let step = length / steps as f64;
        (0..steps).map(|i| transfer_slowness((i as f64 + 0.5) * step, &[[0.0, length]], transfer_meters) * step).sum()
    }

    #[test]
    fn a_long_transfer_lasts_as_long_as_the_cap_and_a_short_one_keeps_the_race_pace() {
        assert!((transfer_time(550_000.0, 40_000.0) - 40_000.0).abs() < 40.0);
        assert!((transfer_time(10_000.0, 40_000.0) - 10_000.0).abs() < 1e-6);
        assert_eq!(transfer_slowness(5.0, &[[10.0, 20.0]], 1.0), 1.0);
    }

    #[test]
    fn the_transfer_background_fades_in_and_out_and_stays_faint_on_short_transfers() {
        let long = [[0.0, 1000.0]];
        let blend = |meters: f64| transfer_blend(meters, &long, 100.0);
        assert_eq!([0.0, 500.0, 1000.0, 1200.0].map(blend), [0.0, 1.0, 0.0, 0.0]);
        assert!(blend(50.0) > 0.0 && blend(50.0) < 0.5 && (blend(50.0) - blend(950.0)).abs() < 1e-9);
        assert!((transfer_blend(5.0, &[[0.0, 10.0]], 100.0) - 0.1).abs() < 1e-9);
    }

    #[test]
    fn even_stages_slow_down_the_short_stages_and_blend_across_the_transfer() {
        let text = r#"{"run":"r","mode":"open","start":[0,0],"path":{"points":[[0,0],[0,1]],"meters":[0,500]},"stops":[],"excluded":[],"transfers":[[100,200]],"stages":[{"name":"1","from":0,"to":100},{"name":"2","from":200,"to":500}],"routedMeters":500}"#;
        let tour = Tour::parse(text).unwrap();
        let slowness = |at: f64, even: f64| stage_slowness(&tour, at, even);
        assert_eq!([50.0, 300.0].map(|at| slowness(at, 0.0)), [1.0, 1.0]);
        // Moyenne 200 m : l'étape de 100 m est √2 fois plus lente, celle de 300 m √1,5 fois plus rapide.
        assert!((slowness(50.0, 0.5) - 2f64.sqrt()).abs() < 1e-9);
        assert!((slowness(300.0, 0.5) - 1.5f64.powf(-0.5)).abs() < 1e-9);
        // Avec evenStages à 1, les deux étapes durent pareil.
        assert!((slowness(50.0, 1.0) * 100.0 - slowness(300.0, 1.0) * 300.0).abs() < 1e-9);
        let middle = slowness(150.0, 0.5);
        assert!(middle < slowness(50.0, 0.5) && middle > slowness(300.0, 0.5));
    }
}
