//! Repères d'une étape (sprints, ascensions) : ils apparaissent au passage de la tête et s'effacent au
//! début du transfert qui suit leur étape. Seuls les lieux restent sur le tracé global.

use skia_safe::{Canvas, Color, Contains, Paint, Point, Rect};

use crate::color::parse_color;
use crate::geo::LonLat;
use crate::mercator::View;
use crate::model::{Theme, Tour};
use crate::route::zoom_ratio;
use crate::sprites::{Fonts, Label};
use dijkstravel_core::tour::{Marker as TourMarker, MarkerKind};

/// Rayon du point d'un repère, en part de la taille de son nom.
const RADIUS_PER_LABEL: f64 = 0.4;
/// Écart entre le repère et son nom, en tailles de police.
const LABEL_OFFSET_EM: f32 = 0.7;
/// Part du transfert suivant pendant laquelle les repères de l'étape s'effacent.
const FADE_SHARE: f64 = 0.3;

struct Marker {
    at: LonLat,
    meters: f64,
    /// Mètres à partir desquels le repère s'efface, et ceux où il a disparu.
    fade: [f64; 2],
    color: Color,
    /// Ascension : disque blanc cerclé de sa couleur, comme le maillot à pois ; sprint : plein.
    ring: bool,
    label: Option<Label>,
    /// Nom à gauche du point : choisi une fois, au passage de la tête, pour ne jamais sauter d'un côté à l'autre.
    left: bool,
}

pub struct Markers {
    markers: Vec<Marker>,
    radius: f32,
    halo_color: Color,
}

impl Markers {
    /// `view_at` : la vue de la caméra quand la tête est à `meters` ; `width` : largeur de l'image.
    pub fn new(tour: &Tour, theme: &Theme, fonts: &Fonts, view_at: impl Fn(f64) -> View, width: f32) -> Self {
        let radius = (theme.stops.marker_label_size * RADIUS_PER_LABEL) as f32;
        let text = parse_color(&theme.hud.text_color);
        let halo = parse_color(&theme.hud.background);
        let size = theme.stops.marker_label_size as f32;
        let total = tour.total_meters();
        let markers = tour
            .markers
            .iter()
            .map(|marker| {
                // Fin de l'étape du repère, puis début de la suivante (fin du transfert).
                let stage =
                    tour.stages.iter().position(|stage| stage.from <= marker.meters && marker.meters <= stage.to);
                let end = stage.map_or(total, |i| tour.stages[i].to);
                let next = stage.and_then(|i| tour.stages.get(i + 1)).map_or(total, |stage| stage.from);
                let fade = if next > end { [end, end + FADE_SHARE * (next - end)] } else { [total, f64::INFINITY] };
                let color = match marker.kind {
                    MarkerKind::Sprint => &theme.stops.sprint_color,
                    MarkerKind::Climb => &theme.stops.climb_color,
                };
                let label = theme.stops.show_labels.then(|| fonts.label(&label_of(marker), size, text, halo)).flatten();
                // À droite, sauf si le nom déborderait déjà de l'écran au passage.
                let left = label.as_ref().is_some_and(|label| {
                    let view = view_at(marker.meters);
                    let ratio = zoom_ratio(view.camera_zoom()) as f32;
                    let x = view.to_screen(marker.at)[0] as f32;
                    x + label_offset(label, radius * ratio) + label.width > width
                });
                Marker {
                    at: marker.at,
                    meters: marker.meters,
                    fade,
                    color: parse_color(color),
                    ring: marker.kind == MarkerKind::Climb,
                    label,
                    left,
                }
            })
            .collect();
        Markers { markers, radius, halo_color: halo }
    }

    /// `pop_meters` : distance sur laquelle un repère atteint grossit avant de revenir à sa taille.
    pub fn draw(&self, canvas: &Canvas, view: &View, screen: Rect, meters: f64, pop_meters: f64, label_opacity: f32) {
        let ratio = zoom_ratio(view.camera_zoom()) as f32;
        let margin = screen.with_outset((300.0, 300.0));
        let mut placed: Vec<Rect> = Vec::new();
        for marker in self.markers.iter().rev() {
            if meters < marker.meters {
                continue;
            }
            let [from, to] = marker.fade;
            let opacity = 1.0 - smoothstep(((meters - from) / (to - from)).clamp(0.0, 1.0));
            let [x, y] = view.to_screen(marker.at);
            let point = Point::new(x as f32, y as f32);
            if opacity <= 0.0 || !margin.contains(point) {
                continue;
            }
            let opacity = opacity as f32;
            let pop = (1.0 - (meters - marker.meters) / pop_meters).max(0.0) as f32;
            let radius = (self.radius + 6.0 * pop) * ratio;
            let mut paint = Paint::default();
            paint.set_anti_alias(true).set_color(self.halo_color).set_alpha_f(0.85 * opacity);
            canvas.draw_circle(point, radius + 3.0 * ratio, &paint);
            paint.set_color(marker.color).set_alpha_f(opacity);
            canvas.draw_circle(point, radius, &paint);
            if marker.ring {
                paint.set_color(Color::WHITE).set_alpha_f(opacity);
                canvas.draw_circle(point, radius * 0.55, &paint);
            }

            let Some(label) = &marker.label else { continue };
            let opacity = opacity * label_opacity;
            if opacity <= 0.0 {
                continue;
            }
            let (w, h) = (label.width, label.height);
            let offset = label_offset(label, radius);
            // Sur son côté, sinon au-dessus s'il chevaucherait un nom plus récent ; il sort de l'écran avec son point.
            let side = if marker.left { point.x - offset - w } else { point.x + offset };
            let candidates = [
                Rect::from_xywh(side, point.y - h / 2.0, w, h),
                Rect::from_xywh(point.x - w / 2.0, point.y - offset - h, w, h),
            ];
            let free = |spot: &Rect| !placed.iter().any(|other| other.intersects(*spot));
            let Some(spot) = candidates.into_iter().find(free) else {
                continue;
            };
            placed.push(spot.with_outset((4.0, 4.0)));
            let mut paint = Paint::default();
            paint.set_alpha_f(opacity);
            canvas.draw_image(&label.image, (spot.left - label.pad, spot.top - label.pad), Some(&paint));
        }
    }
}

/// Écart entre le centre du point et son nom.
fn label_offset(label: &Label, radius: f32) -> f32 {
    LABEL_OFFSET_EM * label.height + radius
}

/// « Sprint · Pouzac », « Col du Tourmalet · HC », « Côte de Billonnière · 4e cat. ».
fn label_of(marker: &TourMarker) -> String {
    match (marker.kind, marker.category.as_deref()) {
        (MarkerKind::Sprint, _) => format!("Sprint · {}", marker.name),
        (MarkerKind::Climb, None) => marker.name.clone(),
        (MarkerKind::Climb, Some("HC")) => format!("{} · HC", marker.name),
        (MarkerKind::Climb, Some("1")) => format!("{} · 1re cat.", marker.name),
        (MarkerKind::Climb, Some(category)) => format!("{} · {category}e cat.", marker.name),
    }
}

fn smoothstep(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_name_the_sprint_town_and_the_climb_category() {
        let marker = |kind, category: Option<&str>| TourMarker {
            kind,
            name: "X".into(),
            category: category.map(str::to_owned),
            at: [0.0, 0.0],
            meters: 0.0,
        };
        assert_eq!(label_of(&marker(MarkerKind::Sprint, None)), "Sprint · X");
        assert_eq!(label_of(&marker(MarkerKind::Climb, Some("HC"))), "X · HC");
        assert_eq!(label_of(&marker(MarkerKind::Climb, Some("1"))), "X · 1re cat.");
        assert_eq!(label_of(&marker(MarkerKind::Climb, Some("3"))), "X · 3e cat.");
    }
}
