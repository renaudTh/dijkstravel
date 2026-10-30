//! Plusieurs rendus à la suite, chacun avec sa tournée et sa caméra. Entre deux segments, la caméra
//! vole de la fin de l'un au début du suivant : elle dézoome en partant déjà vers la ville suivante,
//! passe en vue d'avion, où s'affichent les noms des villes, puis redescend. Ni trajet ni distance.

use skia_safe::{Canvas, Color, Paint, Rect};

use crate::frame::Camera;
use crate::geo::LonLat;
use crate::mercator::{View, project, unproject};
use crate::model::Video;
use crate::render::{Frames, Renderer};
use crate::sprites::{Fonts, Label};

/// Taille du nom des villes sur le plan large, en pixels.
const CITY_LABEL_SIZE: f32 = 40.0;
const CITY_DOT_RADIUS: f32 = 9.0;
/// La ville où l'on va : en blanc comme les autres, plus grande ; les autres un peu estompées.
const NEXT_CITY_LABEL_SIZE: f32 = 48.0;
const OTHER_CITY_OPACITY: f32 = 0.7;
/// Courbure du vol (`rho` de van Wijk et Nuij, `curve` du `flyTo` de MapLibre, qui prend 1,42) : plus
/// elle est forte, plus la caméra monte avant de traverser.
const FLIGHT_CURVE: f64 = 1.6;
/// Zooms entre lesquels les noms des villes apparaissent : invisibles à l'échelle d'une agglomération,
/// entiers en vue d'avion.
const CITY_LABELS_ZOOM: (f64, f64) = (10.0, 8.5);
/// Part du vol sur laquelle s'estompent le tracé de l'étape quittée, et les cartes d'infos.
const TRACE_FADE: f64 = 0.45;
const HUD_FADE: f64 = 0.25;
/// Pixels du petit côté de l'écran, pour la largeur de vue du vol (comme le cadrage de `frame`).
const SCREEN_PIXELS: f64 = 1080.0;

/// Une ville du plan large : là où commence son segment.
pub struct City {
    pub at: LonLat,
    pub name: String,
}

struct CityMark {
    at: LonLat,
    label: Option<Label>,
    highlighted: Option<Label>,
}

pub struct Montage {
    parts: Vec<Renderer>,
    /// Première image de chaque segment dans la vidéo.
    starts: Vec<u32>,
    /// Images de chaque vol entre deux segments.
    transition: u32,
    cities: Vec<CityMark>,
}

impl Montage {
    /// `parts` : au moins un rendu, tous au même format et au même thème ; `cities` : une par segment.
    pub fn new(mut parts: Vec<Renderer>, cities: Vec<City>, transition: u32) -> Montage {
        assert!(!parts.is_empty(), "un montage sans segment");
        // Les noms restent jusqu'au vol, qui les estompe avec le tracé.
        for part in &mut parts {
            part.fade_labels_at_outro = false;
        }
        let halo = Color::from_argb(220, 0, 0, 0);
        let fonts = Fonts::new();
        let cities = cities
            .into_iter()
            .map(|city| CityMark {
                at: city.at,
                label: fonts.label(&city.name, CITY_LABEL_SIZE, Color::WHITE, halo),
                highlighted: fonts.label(&city.name, NEXT_CITY_LABEL_SIZE, Color::WHITE, halo),
            })
            .collect();
        let starts = layout(&parts.iter().map(Frames::frame_count).collect::<Vec<_>>(), transition);
        Montage { parts, starts, transition, cities }
    }

    /// Où tombe `frame` : dans le segment `index`, ou dans le saut qui le suit.
    fn locate(&self, frame: u32) -> Place {
        let index = self.starts.partition_point(|&start| start <= frame).saturating_sub(1);
        let local = frame - self.starts[index];
        let count = self.parts[index].frame_count();
        if local < count || index + 1 == self.parts.len() {
            Place::Segment { index, local: local.min(count.saturating_sub(1)) }
        } else {
            Place::Jump { from: index, progress: progress_of(local - count, self.transition) }
        }
    }

    /// La caméra du vol après le segment `from`, et le segment dont le fond se dessine : celui qu'on
    /// quitte jusqu'au milieu du vol, en vue d'avion, puis celui où l'on arrive.
    fn jump_camera(&self, from: usize, progress: f64) -> (usize, Camera) {
        let t = ease_flight(progress);
        let camera = fly(self.parts[from].last_camera(), self.parts[from + 1].first_camera(), t);
        (if progress < 0.5 { from } else { from + 1 }, camera)
    }

    /// Le point et le nom de chaque ville ; celle de `next` plus grande, les autres estompées.
    fn draw_cities(&self, canvas: &Canvas, camera: Camera, next: usize, opacity: f32) {
        let video = self.video();
        let view = View::new(camera.center, camera.zoom, camera.zoom, video.width, video.height);
        let mut dot = Paint::default();
        dot.set_anti_alias(true).set_color(Color::WHITE);
        let mut halo = Paint::default();
        halo.set_anti_alias(true).set_color(Color::from_argb(220, 0, 0, 0));
        let mut text = Paint::default();
        let mut placed: Vec<Rect> = Vec::new();
        for (i, city) in self.cities.iter().enumerate() {
            let [x, y] = view.to_screen(city.at);
            let (x, y, current) = (x as f32, y as f32, i == next);
            let alpha = opacity * if current { 1.0 } else { OTHER_CITY_OPACITY };
            let radius = CITY_DOT_RADIUS * if current { 1.5 } else { 1.0 };
            halo.set_alpha_f(alpha * 0.86);
            canvas.draw_circle((x, y), radius + 2.5, &halo);
            dot.set_alpha_f(alpha);
            canvas.draw_circle((x, y), radius, &dot);
            text.set_alpha_f(alpha);
            let Some(label) = (if current { &city.highlighted } else { &city.label }) else { continue };
            // À droite du point, sinon à gauche s'il sort de l'écran ou chevauche un nom déjà posé.
            let gap = CITY_DOT_RADIUS * 2.0;
            let (top, w, h) = (y - label.height / 2.0, label.width, label.height);
            let right = Rect::from_xywh(x + gap, top, w, h);
            let left = Rect::from_xywh(x - gap - w, top, w, h);
            let fits = |spot: Rect| {
                spot.left >= 0.0
                    && spot.right <= video.width as f32
                    && !placed.iter().any(|other| other.intersects(spot))
            };
            let spot = if fits(right) || (!fits(left) && left.left < 0.0) { right } else { left };
            placed.push(spot);
            canvas.draw_image(&label.image, (spot.left - label.pad, spot.top - label.pad), Some(&text));
        }
    }
}

enum Place {
    Segment { index: usize, local: u32 },
    Jump { from: usize, progress: f64 },
}

impl Frames for Montage {
    fn video(&self) -> &Video {
        self.parts[0].video()
    }

    fn frame_count(&self) -> u32 {
        let last = self.parts.len() - 1;
        self.starts[last] + self.parts[last].frame_count()
    }

    fn needed_tiles(&self, frames: std::ops::Range<u32>) -> Vec<String> {
        let mut tiles = std::collections::BTreeSet::new();
        for (index, (part, &start)) in self.parts.iter().zip(&self.starts).enumerate() {
            let count = part.frame_count();
            let local = frames.start.saturating_sub(start).min(count)..frames.end.saturating_sub(start).min(count);
            tiles.extend(part.needed_tiles(local));
            if index + 1 < self.parts.len() {
                let jump = start + count..start + count + self.transition;
                for frame in frames.start.max(jump.start)..frames.end.min(jump.end) {
                    let (shown, camera) = self.jump_camera(index, progress_of(frame - jump.start, self.transition));
                    tiles.extend(self.parts[shown].tiles_at(camera));
                }
            }
        }
        tiles.into_iter().collect()
    }

    fn draw(&self, canvas: &Canvas, frame: u32, with_hud: bool) {
        match self.locate(frame) {
            Place::Segment { index, local } => self.parts[index].draw(canvas, local, with_hud),
            Place::Jump { from, progress } => {
                let (shown, camera) = self.jump_camera(from, progress);
                // Le tracé et les lieux de l'étape quittée s'estompent en partant ; la suivante n'en a pas encore.
                let leaving = if shown == from { fade_out(progress, TRACE_FADE) } else { 0.0 };
                self.parts[shown].draw_view(canvas, camera, leaving);
                let (hidden, full) = CITY_LABELS_ZOOM;
                let opacity = ((hidden - camera.zoom) / (hidden - full)).clamp(0.0, 1.0) as f32;
                if opacity > 0.0 {
                    self.draw_cities(canvas, camera, from + 1, opacity);
                }
                if with_hud {
                    // La carte d'infos quittée s'efface au départ, la suivante apparaît à l'arrivée.
                    let last = self.parts[from].frame_count().saturating_sub(1);
                    let cards =
                        [(from, last, fade_out(progress, HUD_FADE)), (from + 1, 0, fade_out(1.0 - progress, HUD_FADE))];
                    for (index, frame, alpha) in cards {
                        if alpha > 0.0 {
                            canvas.save_layer_alpha_f(None, alpha);
                            self.parts[index].draw_hud(canvas, frame);
                            canvas.restore();
                        }
                    }
                }
            }
        }
    }
}

/// Première image de chaque segment, de `counts` images, séparés par `transition` images de saut.
fn layout(counts: &[u32], transition: u32) -> Vec<u32> {
    let mut starts = Vec::with_capacity(counts.len());
    let mut next = 0;
    for count in counts {
        starts.push(next);
        next += count + transition;
    }
    starts
}

/// 1 au début du vol, 0 passé la part `span`, en douceur.
fn fade_out(progress: f64, span: f64) -> f32 {
    let t = (progress / span).clamp(0.0, 1.0);
    (1.0 - t * t * (3.0 - 2.0 * t)) as f32
}

fn progress_of(index: u32, count: u32) -> f64 {
    if count <= 1 { 1.0 } else { f64::from(index) / f64::from(count - 1) }
}

/// Départ avec de l'élan (vitesse 1, la caméra ne traîne pas sur la ville quittée), arrivée en douceur
/// (vitesse 0) : moyenne d'une courbe qui ralentit et d'une qui accélère puis ralentit.
fn ease_flight(t: f64) -> f64 {
    let out = 1.0 - (1.0 - t).powi(2);
    let in_out = if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 };
    (out + in_out) / 2.0
}

/// Vol de `from` (t = 0) à `to` (t = 1) : zoom et déplacement mêlés sur le chemin le plus court perçu
/// (van Wijk et Nuij, « Smooth and efficient zooming and panning », l'`interpolateZoom` de d3).
fn fly(from: Camera, to: Camera, t: f64) -> Camera {
    let rho = FLIGHT_CURVE;
    let (rho2, rho4) = (rho * rho, rho.powi(4));
    // Vue : centre en pixels monde au zoom 0, largeur visible dans ces mêmes pixels.
    let width = |zoom: f64| SCREEN_PIXELS / zoom.exp2();
    let ([x0, y0], [x1, y1]) = (project(from.center, 0.0), project(to.center, 0.0));
    let (w0, w1) = (width(from.zoom), width(to.zoom));
    let (dx, dy) = (x1 - x0, y1 - y0);
    let d2 = dx * dx + dy * dy;
    let (u, w) = if d2 < 1e-12 {
        // Sur place : un simple zoom, exponentiel.
        (t, w0 * (w1 / w0).powf(t))
    } else {
        let d1 = d2.sqrt();
        let b0 = (w1 * w1 - w0 * w0 + rho4 * d2) / (2.0 * w0 * rho2 * d1);
        let b1 = (w1 * w1 - w0 * w0 - rho4 * d2) / (2.0 * w1 * rho2 * d1);
        let r0 = ((b0 * b0 + 1.0).sqrt() - b0).ln();
        let r1 = ((b1 * b1 + 1.0).sqrt() - b1).ln();
        let s = t * (r1 - r0) / rho;
        let u = w0 / (rho2 * d1) * (r0.cosh() * (rho * s + r0).tanh() - r0.sinh());
        (u, w0 * r0.cosh() / (rho * s + r0).cosh())
    };
    Camera { center: unproject([x0 + u * dx, y0 + u * dy], 0.0), zoom: (SCREEN_PIXELS / w).log2() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_separated_by_the_jump() {
        assert_eq!(layout(&[240, 240, 210], 90), vec![0, 330, 660]);
        assert_eq!(layout(&[90], 90), vec![0]);
    }

    #[test]
    fn a_flight_leaves_with_momentum_and_lands_softly() {
        assert_eq!((ease_flight(0.0), ease_flight(1.0)), (0.0, 1.0));
        let speed = |t: f64| (ease_flight(t + 1e-6) - ease_flight(t)) / 1e-6;
        assert!((speed(0.0) - 1.0).abs() < 1e-3 && speed(1.0 - 1e-6).abs() < 1e-3);
    }

    #[test]
    fn a_flight_climbs_between_two_cities_and_lands_on_the_next() {
        let paris = Camera { center: [2.40, 48.85], zoom: 14.5 };
        let lyon = Camera { center: [4.86, 45.75], zoom: 14.5 };
        let close = |a: Camera, b: Camera| {
            (a.center[0] - b.center[0]).abs() < 1e-6
                && (a.center[1] - b.center[1]).abs() < 1e-6
                && (a.zoom - b.zoom).abs() < 1e-6
        };
        assert!(close(fly(paris, lyon, 0.0), paris));
        assert!(close(fly(paris, lyon, 1.0), lyon));
        // À mi-vol, en vue d'avion : les deux villes tiennent à l'écran.
        let middle = fly(paris, lyon, 0.5);
        assert!(middle.zoom < 7.5, "{middle:?}");
        // Le dézoom commence avant que la caméra ait beaucoup avancé.
        let early = fly(paris, lyon, 0.15);
        assert!(early.zoom < 12.0 && (early.center[0] - paris.center[0]) < 0.3, "{early:?}");
    }
}
