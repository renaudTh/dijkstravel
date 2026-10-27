//! Lieux de la tournée : couches `stops`, `stops-core`, `stops-image`, `stops-label` et `stops-icon` de
//! l'ancien rendu MapLibre, dans cet ordre.

use skia_safe::gradient::{Colors, Gradient, Interpolation, shaders::radial_gradient};
use skia_safe::{Canvas, Color, Color4f, Contains, Image, Paint, Point, Rect, TileMode};

use crate::color::parse_color;
use crate::geo::LonLat;
use crate::mercator::View;
use crate::model::{StopsTheme, Theme};
use crate::route::{draw_icon, zoom_ratio};
use crate::sprites::{Fonts, Label, place_image};

/// Écart entre le lieu et son nom (`text-radial-offset`), en tailles de police.
const LABEL_OFFSET_EM: f32 = 0.8;
/// Marge de collision entre deux noms (`text-padding` par défaut).
const LABEL_PADDING: f32 = 2.0;

struct Stop {
    at: LonLat,
    meters: f64,
    image: Option<Image>,
    label: Option<Label>,
}

pub struct Stops {
    stops: Vec<Stop>,
    theme: StopsTheme,
    width: f64,
    core_ratio: f64,
    core_blur_ratio: f64,
    route_color: Color,
    core_color: Color,
    unvisited_color: Color,
    emoji: Option<Image>,
}

/// État d'une image : ce qui change d'une image à l'autre pour les lieux.
pub struct StopsFrame {
    /// Les `visited` premiers lieux sont atteints.
    pub visited: usize,
    pub meters: f64,
    /// Distance sur laquelle un lieu atteint « pope » avant de revenir à sa taille.
    pub pop_meters: f64,
    /// Opacité des noms : effacés pendant le dézoom final.
    pub label_opacity: f32,
}

impl Stops {
    pub fn new(input: &crate::model::Input, tour: &crate::model::Tour, theme: &Theme, fonts: &Fonts) -> Self {
        let stops_theme = &theme.stops;
        let text = parse_color(&theme.hud.text_color);
        let halo = parse_color(&theme.hud.background);
        let stops = tour
            .stops
            .iter()
            .map(|stop| Stop {
                at: stop.at,
                meters: stop.meters,
                // Dessinée en double résolution, comme pour MapLibre.
                image: input
                    .images
                    .get(&stop.id)
                    .and_then(|file| place_image(file, (stops_theme.image_size * 2.0).round() as i32, true)),
                // « Agen · Lot-et-Garonne (47) » → « Agen » : le reste encombre la carte.
                label: stops_theme
                    .show_labels
                    .then(|| {
                        let name = stop.name.split(" · ").next().unwrap_or(&stop.name);
                        fonts.label(name, stops_theme.label_size as f32, text, halo)
                    })
                    .flatten(),
            })
            .collect();
        Stops {
            stops,
            theme: stops_theme.clone(),
            width: theme.route.width,
            core_ratio: theme.route.core_ratio,
            core_blur_ratio: theme.route.core_blur_ratio,
            route_color: parse_color(&theme.route.color),
            core_color: parse_color(&theme.route.core_color),
            unvisited_color: parse_color(&stops_theme.unvisited_color),
            emoji: fonts.emoji(&stops_theme.icon),
        }
    }

    /// Lieu qui porte l'emoji du thème : il remplace son point.
    fn has_emoji(&self, stop: &Stop, visited: bool) -> bool {
        self.emoji.is_some() && visited && !(self.theme.show_image_on_map && stop.image.is_some())
    }

    pub fn draw(&self, canvas: &Canvas, view: &View, screen: Rect, frame: &StopsFrame) {
        let ratio = zoom_ratio(view.camera_zoom()) as f32;
        let margin = screen.with_outset((200.0, 200.0));
        // Lieux à l'écran, avec leur état.
        let shown: Vec<(usize, &Stop, Point, bool, f32)> = self
            .stops
            .iter()
            .enumerate()
            .filter_map(|(index, stop)| {
                let [x, y] = view.to_screen(stop.at);
                let point = Point::new(x as f32, y as f32);
                margin.contains(point).then(|| {
                    let visited = index < frame.visited;
                    let pop =
                        if visited { (1.0 - (frame.meters - stop.meters) / frame.pop_meters).max(0.0) } else { 0.0 };
                    (index, stop, point, visited, pop as f32)
                })
            })
            .collect();

        let visited_radius =
            |pop: f32| self.width as f32 * self.theme.visited_ratio as f32 + self.theme.pop_radius as f32 * pop;
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        for &(_, stop, point, visited, pop) in &shown {
            if self.has_emoji(stop, visited) {
                continue;
            }
            let (color, radius) = if visited {
                (self.route_color, visited_radius(pop))
            } else {
                (self.unvisited_color, self.theme.unvisited_radius as f32)
            };
            paint.set_color(color);
            canvas.draw_circle(point, radius * ratio, &paint);
        }
        for &(_, stop, point, visited, pop) in &shown {
            if visited && !self.has_emoji(stop, visited) {
                let radius = self.core_ratio as f32 * visited_radius(pop) * ratio;
                blurred_circle(canvas, point, radius, self.core_color, self.core_blur_ratio as f32);
            }
        }
        if self.theme.show_image_on_map {
            // Taille fixe (lisible à toutes les échelles), grossie au « pop » puis revenue ; posée sur le lieu.
            for &(_, stop, point, visited, pop) in &by_screen_y(&shown) {
                let (Some(image), true) = (&stop.image, visited) else { continue };
                let size = self.theme.image_size as f32 * (1.0 + 0.6 * pop);
                draw_icon(canvas, image, Rect::from_xywh(point.x - size / 2.0, point.y - size, size, size), 1.0);
            }
        }
        if self.theme.show_labels && frame.label_opacity > 0.0 {
            self.draw_labels(canvas, &shown, frame.label_opacity);
        }
        if let Some(emoji) = &self.emoji {
            // L'emoji grossit au « pop » puis revient à sa taille ; posé au-dessus du point du lieu.
            for &(_, stop, point, visited, pop) in &by_screen_y(&shown) {
                if self.has_emoji(stop, visited) {
                    let size = self.theme.icon_size as f32 * (1.0 + 0.8 * pop) * ratio;
                    draw_icon(canvas, emoji, Rect::from_xywh(point.x - size / 2.0, point.y - size, size, size), 1.0);
                }
            }
        }
    }

    /// Noms des lieux atteints, le dernier atteint d'abord : un nom qui en chevaucherait un déjà placé
    /// essaie dessous, puis à gauche, puis à droite du lieu, sinon n'est pas écrit.
    fn draw_labels(&self, canvas: &Canvas, shown: &[(usize, &Stop, Point, bool, f32)], opacity: f32) {
        let offset = LABEL_OFFSET_EM * self.theme.label_size as f32;
        let mut placed: Vec<Rect> = Vec::new();
        let mut order: Vec<_> = shown.iter().filter(|s| s.3 && s.1.label.is_some()).collect();
        order.sort_by_key(|s| std::cmp::Reverse(s.0));
        for &(_, stop, point, ..) in order {
            let Some(label) = &stop.label else { continue };
            let (w, h) = (label.width, label.height);
            let candidates = [
                // `top` : sous le lieu.
                Rect::from_xywh(point.x - w / 2.0, point.y + offset, w, h),
                // `right` : à gauche du lieu.
                Rect::from_xywh(point.x - offset - w, point.y - h / 2.0, w, h),
                // `left` : à droite du lieu.
                Rect::from_xywh(point.x + offset, point.y - h / 2.0, w, h),
            ];
            let free = candidates.into_iter().find(|candidate| {
                let padded = candidate.with_outset((LABEL_PADDING, LABEL_PADDING));
                !placed.iter().any(|other| other.intersects(padded))
            });
            let Some(spot) = free else { continue };
            placed.push(spot.with_outset((LABEL_PADDING, LABEL_PADDING)));
            let mut paint = Paint::default();
            paint.set_alpha_f(opacity);
            canvas.draw_image(&label.image, (spot.left - label.pad, spot.top - label.pad), Some(&paint));
        }
    }
}

/// Icônes qui se chevauchent : MapLibre dessine celle du bas de l'écran par-dessus.
fn by_screen_y<'a>(shown: &[(usize, &'a Stop, Point, bool, f32)]) -> Vec<(usize, &'a Stop, Point, bool, f32)> {
    let mut sorted = shown.to_vec();
    sorted.sort_by(|a, b| a.2.y.total_cmp(&b.2.y));
    sorted
}

/// `circle-blur` : opacité en `smoothstep` sur la dernière fraction `blur` du rayon.
fn blurred_circle(canvas: &Canvas, center: Point, radius: f32, color: Color, blur: f32) {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    let blur = blur.max(1.0 / radius.max(1e-3));
    if blur * radius < 1.0 || radius <= 0.0 {
        paint.set_color(color);
        canvas.draw_circle(center, radius, &paint);
        return;
    }
    const STEPS: usize = 8;
    let start = (1.0 - blur).max(0.0);
    let positions: Vec<f32> = (0..=STEPS).map(|k| start + (1.0 - start) * k as f32 / STEPS as f32).collect();
    let colors: Vec<Color4f> = positions
        .iter()
        .map(|&p| {
            let t = ((1.0 - p) / blur).clamp(0.0, 1.0);
            let mut faded = Color4f::from(color);
            faded.a *= t * t * (3.0 - 2.0 * t);
            faded
        })
        .collect();
    let gradient =
        Gradient::new(Colors::new(&colors, Some(&positions), TileMode::Clamp, None), Interpolation::default());
    let shader = radial_gradient((center, radius), &gradient, None);
    paint.set_shader(shader);
    canvas.draw_circle(center, radius, &paint);
}
