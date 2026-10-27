//! Tracé parcouru (`traced-stripe`, `traced`, `traced-core`) et tête du tracé (`head`), comme les couches
//! de `src/render/page.ts`.

use skia_safe::canvas::SaveLayerRec;
use skia_safe::{BlendMode, Canvas, Color, Contains, Image, Paint, PathBuilder, Point, Rect, paint};

use crate::color::parse_color;
use crate::geo::{LonLat, MeasuredPath, split_crossings};
use crate::mercator::View;
use crate::model::RouteTheme;

/// Points du tracé dessiné au plus : renvoyer un million de points à chaque image coûterait trop cher.
const MAX_DRAWN_POINTS: f64 = 20_000.0;
/// Tailles pleines au zoom d'une région (Bretagne ≈ 9), réduites au quart au zoom d'un pays (France ≈ 5).
const FULL_SIZE_ZOOM: f64 = 9.0;
const SMALL_SIZE_ZOOM: f64 = 5.0;
const SMALL_SIZE_RATIO: f64 = 0.25;
/// Traits superposés pour reproduire le dégradé linéaire de `line-blur`.
const BLUR_STEPS: usize = 8;

/// `byZoom` de `page.ts` : facteur appliqué aux tailles selon le zoom de l'image.
pub fn zoom_ratio(zoom: f64) -> f64 {
    let t = ((zoom - SMALL_SIZE_ZOOM) / (FULL_SIZE_ZOOM - SMALL_SIZE_ZOOM)).clamp(0.0, 1.0);
    SMALL_SIZE_RATIO + (1.0 - SMALL_SIZE_RATIO) * t
}

pub struct Route {
    /// Tracé allégé pour le dessin seulement ; distances d'origine conservées.
    drawn: MeasuredPath,
    ferries: Vec<[f64; 2]>,
    theme: RouteTheme,
    colors: Colors,
    head_icon: Option<Image>,
}

struct Colors {
    route: Color,
    core: Color,
    stripe: Color,
    ferry: Color,
    head: Color,
}

impl Route {
    pub fn new(path: &MeasuredPath, ferries: &[[f64; 2]], theme: &RouteTheme, head_icon: Option<Image>) -> Self {
        Route {
            drawn: path.decimate(path.total_meters() / MAX_DRAWN_POINTS),
            ferries: ferries.to_vec(),
            colors: Colors {
                route: parse_color(&theme.color),
                core: parse_color(&theme.core_color),
                stripe: parse_color(&theme.stripe_color),
                ferry: parse_color(&theme.ferry_color),
                head: parse_color(&theme.head_color),
            },
            theme: theme.clone(),
            head_icon,
        }
    }

    pub fn draw(&self, canvas: &Canvas, view: &View, screen: Rect, meters: f64) {
        if meters <= 0.0 {
            return;
        }
        let ratio = zoom_ratio(view.camera_zoom());
        let width = self.theme.width * ratio;
        let pieces: Vec<(Vec<Point>, bool)> = split_crossings(&self.drawn, &self.ferries, meters)
            .into_iter()
            .map(|piece| (screen_line(&piece.points, view), piece.ferry))
            .collect();
        // Marge : une extrémité arrondie hors de l'écran reste dessinée entière.
        let visible = screen.with_outset((width as f32 + 4.0, width as f32 + 4.0));
        let route_color = |ferry: bool| if ferry { self.colors.ferry } else { self.colors.route };
        if self.theme.stripes {
            // Deux traits de 2/3 décalés d'1/6 qui se chevauchent sous la bande centrale : aucun liseré
            // de fond ne passe entre deux bandes voisines.
            let stripe_color = |ferry: bool| if ferry { self.colors.ferry } else { self.colors.stripe };
            for (points, ferry) in &pieces {
                let path = visible_path(&offset_line(points, (width / 6.0) as f32), visible);
                stroke(canvas, &path, (width * 2.0 / 3.0) as f32, stripe_color(*ferry));
            }
            for (points, ferry) in &pieces {
                let path = visible_path(&offset_line(points, (-width / 6.0) as f32), visible);
                stroke(canvas, &path, (width * 2.0 / 3.0) as f32, route_color(*ferry));
            }
            for (points, _) in &pieces {
                stroke(canvas, &visible_path(points, visible), (width / 3.0) as f32, self.colors.core);
            }
        } else {
            for (points, ferry) in &pieces {
                stroke(canvas, &visible_path(points, visible), width as f32, route_color(*ferry));
            }
            let core_width = width * self.theme.core_ratio;
            let blur = core_width * self.theme.core_blur_ratio;
            for (points, _) in &pieces {
                blurred_stroke(
                    canvas,
                    &visible_path(points, visible),
                    core_width as f32,
                    blur as f32,
                    self.colors.core,
                );
            }
        }
    }

    /// Tête : seulement pendant le parcours (ni avant le départ, ni sur la vue d'ensemble finale).
    pub fn draw_head(&self, canvas: &Canvas, view: &View, meters: f64) {
        if self.theme.head_radius == 0.0 {
            return;
        }
        let ratio = zoom_ratio(view.camera_zoom());
        let [x, y] = view.to_screen(self.drawn.point_at(meters));
        let center = Point::new(x as f32, y as f32);
        if let Some(icon) = &self.head_icon {
            // Diamètre : deux fois `headRadius`, comme le point qu'il remplace.
            let size = (self.theme.head_radius * 2.0 * ratio) as f32;
            let dest = Rect::from_xywh(center.x - size / 2.0, center.y - size / 2.0, size, size);
            draw_icon(canvas, icon, dest, 1.0);
            return;
        }
        let radius = (self.theme.head_radius * ratio) as f32;
        let ring = (self.theme.width / 4.0 * ratio) as f32;
        let mut paint = Paint::default();
        paint.set_anti_alias(true).set_color(self.colors.route);
        canvas.draw_circle(center, radius + ring, &paint);
        paint.set_color(self.colors.head);
        canvas.draw_circle(center, radius, &paint);
    }
}

/// Icône (emoji, image) dessinée dans `dest`, filtrée en réduction comme en agrandissement.
pub fn draw_icon(canvas: &Canvas, image: &Image, dest: Rect, opacity: f32) {
    let mut paint = Paint::default();
    paint.set_alpha_f(opacity);
    let sampling = skia_safe::SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::Linear);
    canvas.draw_image_rect_with_sampling_options(image, None, dest, sampling, &paint);
}

/// Tracé à l'écran, sans les points à moins d'un quart de pixel du précédent.
fn screen_line(points: &[LonLat], view: &View) -> Vec<Point> {
    let mut line: Vec<Point> = Vec::with_capacity(points.len());
    let last = points.len().saturating_sub(1);
    for (i, &point) in points.iter().enumerate() {
        let [x, y] = view.to_screen(point);
        let p = Point::new(x as f32, y as f32);
        match line.last() {
            Some(previous) if i != last && (p - *previous).length() < 0.25 => {}
            _ => line.push(p),
        }
    }
    line
}

/// Ligne décalée de `distance` pixels vers la droite du sens de parcours (`line-offset`), jointures en
/// onglet bornées.
fn offset_line(points: &[Point], distance: f32) -> Vec<Point> {
    let normal = |a: Point, b: Point| {
        let d = b - a;
        let length = d.length();
        if length == 0.0 { Point::new(0.0, 0.0) } else { Point::new(-d.y / length, d.x / length) }
    };
    let count = points.len();
    (0..count)
        .map(|i| {
            let before = (i > 0).then(|| normal(points[i - 1], points[i]));
            let after = (i + 1 < count).then(|| normal(points[i], points[i + 1]));
            let offset = match (before, after) {
                (Some(a), Some(b)) => {
                    let sum = a + b;
                    let length = sum.length();
                    if length < 1e-3 {
                        b * distance
                    } else {
                        let miter = sum * (1.0 / length);
                        let cos = miter.dot(b).max(0.5);
                        miter * (distance / cos)
                    }
                }
                (Some(n), None) | (None, Some(n)) => n * distance,
                (None, None) => Point::new(0.0, 0.0),
            };
            points[i] + offset
        })
        .collect()
}

/// Seuls les segments qui touchent `visible` : le reste du tracé ne coûte rien à tracer.
fn visible_path(points: &[Point], visible: Rect) -> skia_safe::Path {
    let mut path = PathBuilder::new();
    let mut open = false;
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let bounds = Rect::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y));
        if !bounds.intersects(visible) && !(bounds.width() == 0.0 && visible.contains(a)) {
            open = false;
            continue;
        }
        if !open {
            path.move_to(a);
            open = true;
        }
        path.line_to(b);
    }
    // Un tracé d'un seul point (tout début du parcours) : un point arrondi.
    if points.len() == 1 && visible.contains(points[0]) {
        path.move_to(points[0]).line_to(points[0]);
    }
    path.detach()
}

fn stroke_paint(width: f32, color: Color) -> Paint {
    let mut paint = Paint::default();
    paint.set_anti_alias(true).set_style(paint::Style::Stroke).set_color(color);
    paint.set_stroke_width(width).set_stroke_cap(paint::Cap::Round).set_stroke_join(paint::Join::Round);
    paint
}

fn stroke(canvas: &Canvas, path: &skia_safe::Path, width: f32, color: Color) {
    canvas.draw_path(path, &stroke_paint(width, color));
}

/// `line-blur` : opacité `(bord − distance) / (flou + 1)` bornée à [0, 1], le bord étant à demi-largeur
/// + 0,5. Somme de traits de largeurs étagées, chacun d'opacité 1 / n, additionnés dans un calque.
fn blurred_stroke(canvas: &Canvas, path: &skia_safe::Path, width: f32, blur: f32, color: Color) {
    if blur < 0.25 {
        stroke(canvas, path, width, color);
        return;
    }
    let outset = width / 2.0 + 0.5;
    let ramp = blur + 1.0;
    canvas.save_layer(&SaveLayerRec::default());
    let step_alpha = 1.0 / BLUR_STEPS as f32;
    for k in 0..BLUR_STEPS {
        let radius = outset - ramp * (k as f32 + 0.5) / BLUR_STEPS as f32;
        if radius <= 0.0 {
            continue;
        }
        let mut paint = stroke_paint(radius * 2.0, color);
        paint.set_alpha_f(step_alpha).set_blend_mode(BlendMode::Plus);
        canvas.draw_path(path, &paint);
    }
    canvas.restore();
}
