//! Une image de la vidéo : fond de carte, tracé, lieux, carte d'infos.

use std::collections::BTreeSet;
use std::sync::Arc;

use skia_safe::{Canvas, EncodedImageFormat, Image, Rect, Surface, surfaces};

use crate::atlas::Atlas;
use crate::frame::{Animation, Moment};
use crate::hud::{Hud, HudFrame};
use crate::model::{Input, Tour};
use crate::route::Route;
use crate::sprites::Fonts;
use crate::stops::{Stops, StopsFrame};

/// Durée de l'effet « pop » d'un lieu atteint, en secondes de vidéo.
const POP_SECONDS: f64 = 0.6;
/// Part de l'outro au bout de laquelle les noms des lieux ont disparu.
const LABEL_FADE_PROGRESS: f64 = 0.5;

pub struct Renderer {
    pub input: Input,
    pub tour: Arc<Tour>,
    pub animation: Animation,
    atlas: Arc<Atlas>,
    route: Route,
    stops: Stops,
    hud: Hud,
}

impl Renderer {
    /// `previous` : un rendu précédent (aperçu de l'éditeur) dont l'atlas est repris s'il ne dépend
    /// d'aucun réglage changé, avec ses morceaux déjà calculés.
    pub fn new(input: Input, tour: Arc<Tour>, previous: Option<&Renderer>) -> Self {
        let theme = &input.theme;
        let animation =
            Animation::new(tour.clone(), &input.video, theme.camera.follow_fraction, theme.camera.stop_slowdown);
        let travel_zoom = animation.travel_zoom();
        let atlas = match previous {
            Some(previous) if previous.atlas.serves(travel_zoom, &input) => previous.atlas.clone(),
            _ => Arc::new(Atlas::new(travel_zoom, &input)),
        };
        let fonts = Fonts::new();
        Renderer {
            route: Route::new(&tour.path, &tour.ferries, &theme.route, fonts.emoji(&theme.route.head_icon)),
            stops: Stops::new(&input, &tour, theme, &fonts),
            hud: Hud::new(
                &theme.hud,
                &input.title,
                &input.imagery.credit,
                &input.hud_fonts,
                &tour.stops.iter().map(|stop| input.images.get(&stop.id).cloned()).collect::<Vec<_>>(),
                input.video.width,
                input.video.height,
            ),
            atlas,
            animation,
            tour,
            input,
        }
    }

    pub fn frame_count(&self) -> u32 {
        self.animation.timeline.frame_count()
    }

    /// Toutes les tuiles que liront les images `frames`, `<source>/<z>/<x>/<y>`.
    pub fn needed_tiles(&self, frames: std::ops::Range<u32>) -> Vec<String> {
        let (width, height) = (self.input.video.width, self.input.video.height);
        let chunks: BTreeSet<_> = frames
            .flat_map(|frame| {
                self.atlas.chunks_for(self.animation.camera_at(self.animation.moment_at(frame)), width, height)
            })
            .collect();
        let tiles: BTreeSet<String> = chunks.into_iter().flat_map(|chunk| self.atlas.tiles_of(chunk)).collect();
        tiles.into_iter().collect()
    }

    /// Surface à la taille de la vidéo, réutilisable d'une image à l'autre.
    pub fn surface(&self) -> Surface {
        let (width, height) = (self.input.video.width, self.input.video.height);
        surfaces::raster_n32_premul((width as i32, height as i32)).expect("surface de l'image")
    }

    /// `with_hud` : sans carte d'infos pour la couverture, sur laquelle l'auteur pose son propre titre.
    pub fn render(&self, frame: u32, with_hud: bool) -> Image {
        let mut surface = self.surface();
        self.draw(surface.canvas(), frame, with_hud);
        surface.image_snapshot()
    }

    /// Dessine l'image `frame` sur `canvas`, qu'elle recouvre entièrement.
    pub fn draw(&self, canvas: &Canvas, frame: u32, with_hud: bool) {
        let (width, height) = (self.input.video.width, self.input.video.height);
        let screen = Rect::from_wh(width as f32, height as f32);
        let moment = self.animation.moment_at(frame);
        let view = self.atlas.view(self.animation.camera_at(moment), width, height);
        self.atlas.draw(canvas, view, width, height);

        let meters = self.animation.meters_of(moment);
        let hud = self.animation.hud_at(meters);
        self.route.draw(canvas, &view, screen, meters);
        if let Moment::Travel { .. } = moment {
            self.route.draw_head(canvas, &view, meters);
        }
        let pop_meters = self.animation.total_meters() / self.input.video.duration_sec * POP_SECONDS;
        let label_opacity = match moment {
            Moment::Outro { progress } => (1.0 - progress / LABEL_FADE_PROGRESS).max(0.0),
            _ => 1.0,
        };
        let stops_frame = StopsFrame { visited: hud.visited, meters, pop_meters, label_opacity: label_opacity as f32 };
        self.stops.draw(canvas, &view, screen, &stops_frame);
        if with_hud {
            let hud_frame = HudFrame {
                meters: hud.meters,
                total_meters: hud.total_meters,
                visited: hud.visited,
                total: hud.total,
                last_stop: hud.last_stop.as_deref(),
                intro: matches!(moment, Moment::Intro { .. }),
            };
            self.hud.draw(canvas, &hud_frame);
        }
    }
}

/// PNG ou JPEG d'après l'extension du fichier.
pub fn encode(image: &Image, file: &str) -> Option<Vec<u8>> {
    let format = if file.ends_with(".png") { EncodedImageFormat::PNG } else { EncodedImageFormat::JPEG };
    image.encode(None, format, 92).map(|data| data.as_bytes().to_vec())
}
