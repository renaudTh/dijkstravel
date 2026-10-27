//! Images dessinées une seule fois puis posées à chaque image : emojis, images des lieux, noms des
//! lieux avec leur halo. Mêmes tailles et mêmes cadrages que `src/render/page.ts`.

use skia_safe::textlayout::{FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, TextAlign, TextStyle};
use skia_safe::{
    AlphaType, Color, ColorType, Data, FontMgr, FontStyle, Image, ImageInfo, Paint, Rect, SamplingOptions, images,
    paint, surfaces,
};

/// Taille de l'emoji dans l'image envoyée à MapLibre ; image dessinée en double résolution.
pub const ICON_PIXELS: f32 = 64.0;

pub struct Fonts {
    collection: FontCollection,
}

impl Default for Fonts {
    fn default() -> Self {
        Self::new()
    }
}

impl Fonts {
    pub fn new() -> Self {
        let mut collection = FontCollection::new();
        collection.set_default_font_manager(FontMgr::new(), None);
        Fonts { collection }
    }

    pub fn paragraph(&self, text: &str, style: &TextStyle, align: TextAlign, width: f32) -> Paragraph {
        let mut paragraph_style = ParagraphStyle::new();
        paragraph_style.set_text_align(align).set_max_lines(1);
        let mut builder = ParagraphBuilder::new(&paragraph_style, self.collection.clone());
        builder.push_style(style);
        builder.add_text(text);
        let mut paragraph = builder.build();
        paragraph.layout(width);
        paragraph
    }

    /// Emoji centré dans un carré de `ICON_PIXELS × 2` pixels, comme `setEmojiImage` : police à 80 % du
    /// carré, ligne de base « middle » abaissée de 5 %.
    pub fn emoji(&self, icon: &str) -> Option<Image> {
        if icon.is_empty() {
            return None;
        }
        let size = ICON_PIXELS * 2.0;
        let mut style = TextStyle::new();
        style.set_font_families(&["Noto Color Emoji"]).set_font_size(size * 0.8).set_color(Color::BLACK);
        let paragraph = self.paragraph(icon, &style, TextAlign::Center, size);
        let mut surface = surfaces::raster_n32_premul((size as i32, size as i32))?;
        // « middle » : milieu du cadratin sur `size / 2 + 5 %` ; la ligne de base est en dessous.
        let metrics = paragraph.get_line_metrics();
        let (ascent, descent) = metrics.first().map(|m| (m.ascent as f32, m.descent as f32)).unwrap_or((0.0, 0.0));
        let baseline = size / 2.0 + size * 0.05 + (ascent - descent) / 2.0;
        paragraph.paint(surface.canvas(), (0.0, baseline - paragraph.alphabetic_baseline()));
        Some(surface.image_snapshot())
    }

    /// Nom d'un lieu (Noto Sans Bold, halo de 2 pixels), avec la marge laissée autour du texte.
    pub fn label(&self, text: &str, size: f32, color: Color, halo: Color) -> Option<Label> {
        const HALO: f32 = 2.0;
        let mut style = TextStyle::new();
        style.set_font_families(&["Noto Sans"]).set_font_size(size).set_font_style(FontStyle::bold());
        let mut fill = Paint::default();
        fill.set_anti_alias(true).set_color(color);
        style.set_foreground_paint(&fill);
        let text_paragraph = self.paragraph(text, &style, TextAlign::Left, f32::INFINITY);
        let mut stroke = Paint::default();
        stroke.set_anti_alias(true).set_color(halo).set_style(paint::Style::Stroke);
        stroke.set_stroke_width(HALO * 2.0).set_stroke_join(paint::Join::Round);
        style.set_foreground_paint(&stroke);
        let halo_paragraph = self.paragraph(text, &style, TextAlign::Left, f32::INFINITY);
        let (width, height) = (text_paragraph.max_intrinsic_width().ceil(), text_paragraph.height().ceil());
        let pad = HALO + 1.0;
        let mut surface = surfaces::raster_n32_premul(((width + 2.0 * pad) as i32, (height + 2.0 * pad) as i32))?;
        halo_paragraph.paint(surface.canvas(), (pad, pad));
        text_paragraph.paint(surface.canvas(), (pad, pad));
        Some(Label { image: surface.image_snapshot(), width, height, pad })
    }
}

pub struct Label {
    pub image: Image,
    /// Boîte du texte seul, halo exclu : c'est elle qui se place et qui entre en collision.
    pub width: f32,
    pub height: f32,
    pub pad: f32,
}

/// Image d'un lieu (blason, logo) dans un carré de `pixels` de côté, entière et centrée ; posée sur le
/// bas (`bottom`, comme `setPlaceImages` pour la carte) ou centrée en hauteur (`object-fit: contain`).
pub fn place_image(file: &str, pixels: i32, bottom: bool) -> Option<Image> {
    let source = if file.to_lowercase().ends_with(".svg") { svg(file, pixels)? } else { raster(file)? };
    let (w, h) = (source.width() as f32, source.height() as f32);
    let scale = (pixels as f32 / w).min(pixels as f32 / h);
    let (width, height) = (w * scale, h * scale);
    let mut surface = surfaces::raster_n32_premul((pixels, pixels))?;
    let top = if bottom { pixels as f32 - height } else { (pixels as f32 - height) / 2.0 };
    let dest = Rect::from_xywh((pixels as f32 - width) / 2.0, top, width, height);
    let sampling = SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::Linear);
    let source = source.with_default_mipmaps().unwrap_or(source);
    surface.canvas().draw_image_rect_with_sampling_options(&source, None, dest, sampling, &Paint::default());
    Some(surface.image_snapshot())
}

fn raster(file: &str) -> Option<Image> {
    Image::from_encoded(Data::new_copy(&std::fs::read(file).ok()?))?.make_raster_image(None, None)
}

/// SVG rendu par resvg à la taille du carré (pas d'agrandissement d'une petite image raster).
fn svg(file: &str, pixels: i32) -> Option<Image> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_data(&std::fs::read(file).ok()?, &options).ok()?;
    let size = tree.size();
    let scale = (pixels as f32 / size.width()).min(pixels as f32 / size.height());
    let (width, height) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width.max(1), height.max(1))?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let info = ImageInfo::new((width as i32, height as i32), ColorType::RGBA8888, AlphaType::Premul, None);
    images::raster_from_data(&info, Data::new_copy(pixmap.data()), width as usize * 4)
}
