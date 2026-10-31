//! Carte d'infos et mention des sources, reproduction du CSS de l'ancien rendu MapLibre : titre,
//! distance parcourue, nombre de lieux, dernier lieu atteint dans sa pastille, image de ce lieu.

use std::cell::RefCell;

use skia_safe::canvas::SaveLayerRec;
use skia_safe::font_style::{Slant, Weight, Width};
use skia_safe::gradient::{Colors, Gradient, Interpolation, shaders::linear_gradient};
use skia_safe::textlayout::{
    FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, TextAlign, TextShadow, TextStyle, TypefaceFontProvider,
};
use skia_safe::{
    Canvas, Color, Color4f, Data, FontMgr, FontStyle, Image, Paint, Point, RRect, Rect, TileMode, Typeface,
    image_filters,
};

use crate::color::parse_color;
use crate::model::HudTheme;
use crate::sprites::place_image;

/// Pastille du dernier lieu : sa police est réduite par pas de 5 % jusqu'à 55 % pour tenir sur une ligne.
const MIN_NAME_SCALE: f32 = 0.55;
const ATTRIBUTION: &str =
    "© OpenStreetMap contributors · © OpenMapTiles · OpenFreeMap · {credit} · Mapzen Terrain Tiles";

pub struct Hud {
    theme: HudTheme,
    title: String,
    attribution: String,
    family: String,
    typefaces: Vec<Typeface>,
    /// Image de chaque lieu (`None` sans image), à la taille de sa boîte.
    stop_images: Vec<Option<Image>>,
    has_images: bool,
    width: f32,
    height: f32,
    colors: HudColors,
}

struct HudColors {
    background: Color,
    text: Color,
    title: Color,
    distance: Color,
    pill_from: Color,
    pill_to: Color,
    pill_text: Color,
}

/// Ce que montre la carte d'infos à une image.
pub struct HudFrame<'a> {
    pub meters: f64,
    pub total_meters: f64,
    pub visited: usize,
    pub total: usize,
    pub last_stop: Option<&'a str>,
    /// Pendant l'intro : chiffres et dernier lieu masqués, la carte garde sa taille.
    pub intro: bool,
    /// Profil d'altitude du parcours (`hud.showProfile`), et la part du tracé parcourue (0 à 1).
    pub profile: Option<(&'a Profile, f64)>,
}

/// Altitudes du parcours à intervalles réguliers le long du tracé, en mètres.
pub struct Profile {
    pub altitudes: Vec<f32>,
}

impl Profile {
    /// Altitude à la part `progress` du tracé (0 à 1), interpolée.
    fn at(&self, progress: f64) -> f32 {
        let last = self.altitudes.len().saturating_sub(1);
        let position = (progress.clamp(0.0, 1.0) * last as f64) as f32;
        let i = (position.floor() as usize).min(last.saturating_sub(1));
        let t = position - i as f32;
        match self.altitudes.get(i + 1) {
            Some(&next) => self.altitudes[i] + (next - self.altitudes[i]) * t,
            None => self.altitudes.get(i).copied().unwrap_or(0.0),
        }
    }
}

thread_local! {
    /// Mise en page du texte, propre à chaque thread (Skia ne la partage pas entre threads).
    static COLLECTION: RefCell<Option<FontCollection>> = const { RefCell::new(None) };
}

impl Hud {
    pub fn new(
        theme: &HudTheme,
        title: &str,
        credit: &str,
        font_files: &[String],
        stop_images: &[Option<String>],
        width: u32,
        height: u32,
    ) -> Self {
        let manager = FontMgr::new();
        let typefaces: Vec<Typeface> = font_files
            .iter()
            .filter_map(|file| manager.new_from_data(Data::new_copy(&std::fs::read(file).ok()?), None))
            .collect();
        let box_side = (theme.size as f32 * 1.9).round() as i32;
        let stop_images: Vec<Option<Image>> = stop_images
            .iter()
            .map(|file| file.as_deref().and_then(|file| place_image(file, box_side, false)))
            .collect();
        Hud {
            theme: theme.clone(),
            title: title.to_owned(),
            attribution: ATTRIBUTION.replace("{credit}", credit),
            family: theme.font.clone(),
            typefaces,
            has_images: stop_images.iter().any(Option::is_some),
            stop_images,
            width: width as f32,
            height: height as f32,
            colors: HudColors {
                background: parse_color(&theme.background),
                text: parse_color(&theme.text_color),
                title: parse_color(&theme.title_color),
                distance: parse_color(&theme.distance_color),
                pill_from: parse_color(&theme.pill_from),
                pill_to: parse_color(&theme.pill_to),
                pill_text: parse_color(&theme.pill_text_color),
            },
        }
    }

    fn with_collection<T>(&self, run: impl FnOnce(&FontCollection) -> T) -> T {
        COLLECTION.with(|cell| {
            let mut cell = cell.borrow_mut();
            let collection = cell.get_or_insert_with(|| {
                let mut provider = TypefaceFontProvider::new();
                for typeface in &self.typefaces {
                    provider.register_typeface(typeface.clone(), Some(self.family.as_str()));
                }
                let mut collection = FontCollection::new();
                collection.set_asset_font_manager(Some(provider.into()));
                // Repli (emoji, caractères absents de la police) : polices du système.
                collection.set_default_font_manager(FontMgr::new(), None);
                collection
            });
            run(collection)
        })
    }

    fn style(&self, size: f32, weight: i32, color: Color) -> TextStyle {
        let mut style = TextStyle::new();
        style.set_font_families(&[self.family.as_str()]).set_font_size(size).set_color(color);
        style.set_font_style(FontStyle::new(Weight::from(weight), Width::NORMAL, Slant::Upright));
        style
    }

    fn paragraph(&self, runs: &[(&str, TextStyle)], width: f32, ellipsis: bool) -> Paragraph {
        self.with_collection(|collection| {
            let mut paragraph_style = ParagraphStyle::new();
            paragraph_style.set_text_align(TextAlign::Left);
            if ellipsis {
                paragraph_style.set_max_lines(1).set_ellipsis("…");
            }
            let mut builder = ParagraphBuilder::new(&paragraph_style, collection.clone());
            for (text, style) in runs {
                builder.push_style(style);
                builder.add_text(text);
                builder.pop();
            }
            let mut paragraph = builder.build();
            paragraph.layout(width);
            paragraph
        })
    }

    pub fn draw(&self, canvas: &Canvas, frame: &HudFrame) {
        self.draw_card(canvas, frame);
        self.draw_attribution(canvas);
    }

    fn draw_card(&self, canvas: &Canvas, frame: &HudFrame) {
        let theme = &self.theme;
        let size = theme.size as f32;
        let portrait = self.height > self.width;
        let (left, top) = if portrait {
            (self.width * 0.05, self.height * PORTRAIT_TOP / 1920.0)
        } else {
            (self.width * 0.03, self.height * 0.03)
        };
        let card_width = self.width - 2.0 * left;
        let (pad_x, pad_y) = (0.8 * size, 0.6 * size);
        let inner = card_width - 2.0 * pad_x;
        let gap = 0.35 * size;
        let faded = |color: Color| color.with_a((0.85 * f32::from(color.a())).round() as u8);

        // Titre, sur plusieurs lignes s'il le faut.
        let title = theme.show_title.then(|| {
            let mut style = self.style(0.6 * size, 700, self.colors.title);
            style.set_letter_spacing(0.08 * 0.6 * size);
            self.paragraph(&[(&self.title.to_uppercase(), style)], inner, false)
        });
        // Distance, distance totale et nombre de lieux, alignés sur la même ligne de base.
        let km = |meters: f64| format!("{} km", group_thousands((meters / 1000.0).round() as i64));
        let counter = self.paragraph(
            &[
                (&km(frame.meters), self.style(1.5 * size, 900, self.colors.distance)),
                (" ", self.style(size, 800, self.colors.text)),
                (&format!("/ {}", km(frame.total_meters)), self.style(0.8 * size, 800, faded(self.colors.text))),
            ],
            f32::INFINITY,
            false,
        );
        let count = theme.show_count.then(|| {
            let text = format!("{} / {} lieux", frame.visited, frame.total);
            self.paragraph(&[(&text, self.style(0.8 * size, 800, faded(self.colors.text)))], f32::INFINITY, false)
        });
        let ascent = counter.alphabetic_baseline().max(count.as_ref().map_or(0.0, |c| c.alphabetic_baseline()));
        let descent = (counter.height() - counter.alphabetic_baseline())
            .max(count.as_ref().map_or(0.0, |c| c.height() - c.alphabetic_baseline()));
        let stats_height = ascent + descent;
        // Dernier lieu : boîte d'image (même vide) puis pastille.
        let image_shown = theme.show_image && self.has_images;
        let image_box = 1.9 * size;
        let pill_max = inner - if image_shown { image_box + 0.4 * size } else { 0.0 };
        let pill = theme.show_last_stop.then(|| self.pill(frame.last_stop.unwrap_or(""), pill_max));
        let row_height = pill.as_ref().map_or(0.0, |p| p.height).max(if image_shown { image_box } else { 0.0 });
        let profile_height = 2.2 * size;

        let mut height = 2.0 * pad_y + stats_height + gap + row_height;
        if theme.show_profile && frame.profile.is_some() {
            height += gap + profile_height;
        }
        if let Some(title) = &title {
            height += title.height() + gap;
        }
        let card = Rect::from_xywh(left, top, card_width, height);
        let rounded = RRect::new_rect_xy(card, 0.8 * size, 0.8 * size);
        canvas.save();
        canvas.clip_rrect(rounded, None, true);
        // `backdrop-filter: blur(6px)` puis le fond à 72 %.
        let blur = image_filters::blur((6.0, 6.0), TileMode::Clamp, None, None);
        canvas.save_layer(&SaveLayerRec::default().bounds(&card).backdrop(blur.as_ref().unwrap()));
        canvas.draw_color(self.colors.background.with_a((0.72 * 255.0_f32).round() as u8), None);
        canvas.restore();
        canvas.restore();

        let x = left + pad_x;
        let mut y = top + pad_y;
        if let Some(title) = &title {
            title.paint(canvas, (x, y));
            y += title.height() + gap;
        }
        if !frame.intro {
            counter.paint(canvas, (x, y + ascent - counter.alphabetic_baseline()));
            if let Some(count) = &count {
                let after = x + counter.max_intrinsic_width() + 0.8 * size;
                count.paint(canvas, (after, y + ascent - count.alphabetic_baseline()));
            }
        }
        y += stats_height + gap;
        if frame.intro {
            return;
        }
        let mut row_x = x;
        if image_shown {
            let image = frame.visited.checked_sub(1).and_then(|i| self.stop_images.get(i)).and_then(Option::as_ref);
            if let Some(image) = image {
                let top = y + (row_height - image_box) / 2.0;
                canvas.draw_image_rect(
                    image,
                    None,
                    Rect::from_xywh(row_x, top, image_box, image_box),
                    &Paint::default(),
                );
            }
            row_x += image_box + 0.4 * size;
        }
        if let Some((profile, progress)) = frame.profile.filter(|_| theme.show_profile) {
            let area = Rect::from_xywh(x, y + row_height + gap, inner, profile_height);
            self.draw_profile(canvas, profile, progress, area);
        }
        if let (Some(pill), Some(_)) = (pill, frame.last_stop) {
            let top = y + (row_height - pill.height) / 2.0;
            let rect = Rect::from_xywh(row_x, top, pill.width, pill.height);
            let colors = [Color4f::from(self.colors.pill_from), Color4f::from(self.colors.pill_to)];
            let gradient = Gradient::new(Colors::new(&colors, None, TileMode::Clamp, None), Interpolation::default());
            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            paint.set_shader(linear_gradient(
                (Point::new(rect.left, 0.0), Point::new(rect.right, 0.0)),
                &gradient,
                None,
            ));
            canvas.draw_rrect(RRect::new_rect_xy(rect, pill.height / 2.0, pill.height / 2.0), &paint);
            pill.text.paint(canvas, (row_x + pill.pad_x, top + pill.pad_y));
        }
    }

    /// Profil d'altitude dans `area` : tout le parcours en filigrane, la part parcourue dans la couleur
    /// de la distance, la tête et son altitude.
    fn draw_profile(&self, canvas: &Canvas, profile: &Profile, progress: f64, area: Rect) {
        let size = self.theme.size as f32;
        let label = self.paragraph(
            &[(
                &format!("{} m", group_thousands(profile.at(progress).round() as i64)),
                self.style(0.6 * size, 800, self.colors.text),
            )],
            f32::INFINITY,
            false,
        );
        // Place à droite pour l'altitude de la tête.
        let graph =
            Rect::from_ltrb(area.left, area.top, area.right - label.max_intrinsic_width() - 0.4 * size, area.bottom);
        let (low, high) = profile.altitudes.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &a| (lo.min(a), hi.max(a)));
        // Au moins 300 m d'écart : une étape plate le reste à l'écran.
        let span = (high - low).max(300.0);
        let floor = low - span * 0.1;
        let point = |t: f32, altitude: f32| {
            Point::new(
                graph.left + t * graph.width(),
                graph.bottom - (altitude - floor) / (span * 1.1) * graph.height(),
            )
        };
        let count = profile.altitudes.len().max(2);
        let area_path = |until: f32| {
            let mut path = skia_safe::PathBuilder::new();
            path.move_to((graph.left, graph.bottom));
            for (i, &altitude) in profile.altitudes.iter().enumerate() {
                let t = i as f32 / (count - 1) as f32;
                if t > until {
                    path.line_to(point(until, profile.at(f64::from(until))));
                    break;
                }
                path.line_to(point(t, altitude));
            }
            path.line_to((graph.left + until * graph.width(), graph.bottom));
            path.close();
            path.detach()
        };
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color(self.colors.text.with_a(56));
        canvas.draw_path(&area_path(1.0), &paint);
        let progress = progress.clamp(0.0, 1.0) as f32;
        paint.set_color(self.colors.distance);
        canvas.draw_path(&area_path(progress), &paint);
        let head = point(progress, profile.at(f64::from(progress)));
        paint.set_color(self.colors.text);
        canvas.draw_circle(head, 0.18 * size, &paint);
        label.paint(
            canvas,
            (area.right - label.max_intrinsic_width(), area.top + (area.height() - label.height()) / 2.0),
        );
    }

    /// Pastille du dernier lieu : `fitOnOneLine` de `page.ts`, puis points de suspension.
    fn pill(&self, name: &str, max_width: f32) -> Pill {
        let size = self.theme.size as f32;
        let layout = |scale: f32, width: f32, ellipsis: bool| {
            let font_size = size * scale;
            let text = self.paragraph(&[(name, self.style(font_size, 800, self.colors.pill_text))], width, ellipsis);
            (font_size, text)
        };
        let fits = |scale: f32| {
            let (font_size, text) = layout(scale, f32::INFINITY, false);
            text.max_intrinsic_width() + 1.4 * font_size <= max_width
        };
        let mut scale = 1.0;
        if !fits(scale) {
            scale = 0.95;
            while scale - 0.05 >= MIN_NAME_SCALE - 1e-6 && !fits(scale) {
                scale -= 0.05;
            }
        }
        let font_size = size * scale;
        let (pad_x, pad_y) = (0.7 * font_size, 0.2 * font_size);
        let (_, text) = layout(scale, (max_width - 2.0 * pad_x).max(1.0), true);
        let text_width = text.max_intrinsic_width().min(max_width - 2.0 * pad_x);
        let width = text_width + 2.0 * pad_x;
        Pill { height: text.height() + 2.0 * pad_y, width, pad_x, pad_y, text }
    }

    fn draw_attribution(&self, canvas: &Canvas) {
        let mut style = self.style(12.0, 700, Color::from_argb((0.7 * 255.0_f32).round() as u8, 255, 255, 255));
        // `text-shadow: 0 0 3px rgb(0 0 0 / 80%)`.
        style.add_shadow(TextShadow::new(Color::from_argb(204, 0, 0, 0), (0.0, 0.0), 1.5));
        let left = self.width * 0.015;
        let paragraph = self.paragraph(&[(&self.attribution, style)], self.width - left, false);
        let bottom = self.height * 0.99;
        paragraph.paint(canvas, (left, bottom - paragraph.height()));
    }
}

struct Pill {
    text: Paragraph,
    width: f32,
    height: f32,
    pad_x: f32,
    pad_y: f32,
}

/// `Intl.NumberFormat('fr-FR')` : milliers séparés par une espace fine insécable.
/// Marge haute en vertical, en pixels d'une vidéo 1080 × 1920 : TikTok pose ses onglets et sa barre de
/// recherche au-dessus de la vidéo.
const PORTRAIT_TOP: f32 = 250.0;

fn group_thousands(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{202F}');
        }
        out.push(digit);
    }
    if value < 0 { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_like_french_intl() {
        assert_eq!(group_thousands(7), "7");
        assert_eq!(group_thousands(1234), "1\u{202F}234");
        assert_eq!(group_thousands(12_345_678), "12\u{202F}345\u{202F}678");
    }
}
