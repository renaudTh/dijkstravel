//! Plusieurs rendus à la suite, chacun avec sa tournée et sa caméra. Entre deux segments, la caméra
//! dézoome jusqu'au plan large (toute la France, par exemple), où s'affichent les noms des villes,
//! puis rezoome sur le début du segment suivant : un saut, sans trajet ni distance entre les deux.

use skia_safe::{Canvas, Color, Paint, Rect};

use crate::color::parse_color;
use crate::frame::{Camera, fit, smoothstep};
use crate::geo::LonLat;
use crate::mercator::{View, project, unproject};
use crate::model::Video;
use crate::render::{Frames, Renderer};
use crate::sprites::{Fonts, Label};

/// Taille du nom des villes sur le plan large, en pixels.
const CITY_LABEL_SIZE: f32 = 40.0;
const CITY_DOT_RADIUS: f32 = 9.0;
/// Part du saut prise par le dézoom, autant par le rezoom ; le reste est une pause sur le plan large.
const ZOOM_SHARE: f64 = 0.4;

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
    /// Images de chaque saut entre deux segments.
    transition: u32,
    wide: Camera,
    cities: Vec<CityMark>,
    /// Point des villes, et celui de la ville où l'on va.
    colors: (Color, Color),
}

impl Montage {
    /// `parts` : au moins un rendu, tous au même format et au même thème ; `cities` : une par segment.
    /// `wide` : les points que cadre le plan large, à défaut ceux de tous les tracés.
    pub fn new(parts: Vec<Renderer>, cities: Vec<City>, wide: Option<Vec<LonLat>>, transition: u32) -> Montage {
        assert!(!parts.is_empty(), "un montage sans segment");
        let wide = match wide {
            Some(points) => fit(points),
            None => fit(parts.iter().flat_map(|part| part.tour.path.points.iter().copied())),
        };
        let (normal, highlight) = (Color::WHITE, parse_color(&parts[0].input.theme.hud.title_color));
        let halo = Color::from_argb(220, 0, 0, 0);
        let fonts = Fonts::new();
        let cities = cities
            .into_iter()
            .map(|city| CityMark {
                at: city.at,
                label: fonts.label(&city.name, CITY_LABEL_SIZE, normal, halo),
                highlighted: fonts.label(&city.name, CITY_LABEL_SIZE, highlight, halo),
            })
            .collect();
        let starts = layout(&parts.iter().map(Frames::frame_count).collect::<Vec<_>>(), transition);
        Montage { parts, starts, transition, wide, cities, colors: (normal, highlight) }
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

    /// La caméra du saut après le segment `from` : dézoom jusqu'au plan large, pause, puis rezoom sur le
    /// suivant. Rend aussi le segment dont le fond se dessine et la part du plan large atteinte (de 0 à 1).
    fn jump_camera(&self, from: usize, progress: f64) -> (usize, Camera, f64) {
        let zoom_out = (progress / ZOOM_SHARE).min(1.0);
        let zoom_in = ((1.0 - progress) / ZOOM_SHARE).min(1.0);
        if progress < 0.5 {
            let t = smoothstep(zoom_out);
            (from, zoom_between(self.parts[from].last_camera(), self.wide, t), t)
        } else {
            let t = smoothstep(zoom_in);
            (from + 1, zoom_between(self.parts[from + 1].first_camera(), self.wide, t), t)
        }
    }

    /// Le point et le nom de chaque ville ; celle de `next` dans la couleur du titre.
    fn draw_cities(&self, canvas: &Canvas, camera: Camera, next: usize, opacity: f32) {
        let video = self.video();
        let view = View::new(camera.center, camera.zoom, camera.zoom, video.width, video.height);
        let mut dot = Paint::default();
        dot.set_anti_alias(true);
        let mut text = Paint::default();
        text.set_alpha_f(opacity);
        let mut placed: Vec<Rect> = Vec::new();
        for (i, city) in self.cities.iter().enumerate() {
            let [x, y] = view.to_screen(city.at);
            let (x, y, current) = (x as f32, y as f32, i == next);
            dot.set_color(if current { self.colors.1 } else { self.colors.0 });
            dot.set_alpha_f(opacity);
            canvas.draw_circle((x, y), CITY_DOT_RADIUS * if current { 1.4 } else { 1.0 }, &dot);
            let Some(label) = (if current { &city.highlighted } else { &city.label }) else { continue };
            // À droite du point, sinon à gauche s'il chevauche un nom déjà posé.
            let gap = CITY_DOT_RADIUS * 2.0;
            let (top, w, h) = (y - label.height / 2.0, label.width, label.height);
            let right = Rect::from_xywh(x + gap, top, w, h);
            let spot = if placed.iter().any(|other| other.intersects(right)) {
                Rect::from_xywh(x - gap - w, top, w, h)
            } else {
                right
            };
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
                    let (shown, camera, _) = self.jump_camera(index, progress_of(frame - jump.start, self.transition));
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
                let (shown, camera, wide) = self.jump_camera(from, progress);
                // Le tracé de la ville quittée reste visible en partant ; la suivante n'en a pas encore.
                self.parts[shown].draw_view(canvas, camera, shown == from);
                // Les noms apparaissent à l'approche du plan large et s'effacent au rezoom.
                let opacity = ((wide - 0.4) / 0.5).clamp(0.0, 1.0) as f32;
                if opacity > 0.0 {
                    self.draw_cities(canvas, camera, from + 1, opacity);
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

fn progress_of(index: u32, count: u32) -> f64 {
    if count <= 1 { 1.0 } else { f64::from(index) / f64::from(count - 1) }
}

/// De `near` (t = 0) à `wide` (t = 1). Le centre glisse pour moitié au rythme de l'échelle, qui garde
/// le point de départ à l'écran pendant que la vue s'élargit, pour moitié au rythme du zoom, qui évite
/// de tout rattraper d'un coup à la fin.
fn zoom_between(near: Camera, wide: Camera, t: f64) -> Camera {
    let zoom = near.zoom + (wide.zoom - near.zoom) * t;
    let scale = |zoom: f64| (-zoom).exp2();
    let span = scale(wide.zoom) - scale(near.zoom);
    let by_scale = if span.abs() < 1e-12 { t } else { ((scale(zoom) - scale(near.zoom)) / span).clamp(0.0, 1.0) };
    let weight = (by_scale + t) / 2.0;
    let (a, b) = (project(near.center, 0.0), project(wide.center, 0.0));
    let center = unproject([a[0] + (b[0] - a[0]) * weight, a[1] + (b[1] - a[1]) * weight], 0.0);
    Camera { center, zoom }
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
    fn a_jump_goes_from_the_city_to_the_wide_shot() {
        let near = Camera { center: [4.85, 45.75], zoom: 14.0 };
        let wide = Camera { center: [2.5, 46.6], zoom: 5.5 };
        let close = |a: Camera, b: Camera| {
            (a.center[0] - b.center[0]).abs() < 1e-9
                && (a.center[1] - b.center[1]).abs() < 1e-9
                && (a.zoom - b.zoom).abs() < 1e-9
        };
        assert!(close(zoom_between(near, wide, 0.0), near));
        assert!(close(zoom_between(near, wide, 1.0), wide));
        // À mi-zoom, le centre n'a pas encore fait la moitié du chemin : la ville de départ reste en vue.
        let middle = zoom_between(near, wide, 0.5);
        assert!((middle.center[0] - near.center[0]).abs() < 0.4 * (near.center[0] - wide.center[0]), "{middle:?}");
    }
}
