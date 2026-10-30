//! Une image de la vidéo : fond de carte, tracé, lieux, carte d'infos.

use std::collections::BTreeSet;
use std::sync::Arc;

/// Une image rendue, à encoder par `encode`.
pub use skia_safe::Image;
use skia_safe::{Canvas, EncodedImageFormat, Rect, Surface, surfaces};

use crate::atlas::Atlas;
use crate::frame::{Animation, Camera, Moment};
use crate::hud::{Hud, HudFrame};
use crate::model::{Input, Tour, Video};
use crate::route::Route;
use crate::sprites::Fonts;
use crate::stops::{Stops, StopsFrame};

/// Durée de l'effet « pop » d'un lieu atteint, en secondes de vidéo.
const POP_SECONDS: f64 = 0.6;
/// Part de l'outro au bout de laquelle les noms des lieux ont disparu.
const LABEL_FADE_PROGRESS: f64 = 0.5;

/// Ce que la vidéo lit d'un rendu : un run seul (`Renderer`) ou plusieurs à la suite (`Montage`).
pub trait Frames: Sync {
    fn video(&self) -> &Video;
    fn frame_count(&self) -> u32;
    /// Toutes les tuiles que liront les images `frames`, `<source>/<z>/<x>/<y>`.
    fn needed_tiles(&self, frames: std::ops::Range<u32>) -> Vec<String>;
    /// Dessine l'image `frame` sur `canvas`, qu'elle recouvre entièrement.
    fn draw(&self, canvas: &Canvas, frame: u32, with_hud: bool);

    /// Surface à la taille de la vidéo, réutilisable d'une image à l'autre.
    fn surface(&self) -> Surface {
        let video = self.video();
        surfaces::raster_n32_premul((video.width as i32, video.height as i32)).expect("surface de l'image")
    }

    /// `with_hud` : sans carte d'infos pour la couverture, sur laquelle l'auteur pose son propre titre.
    fn render(&self, frame: u32, with_hud: bool) -> Image {
        let mut surface = self.surface();
        self.draw(surface.canvas(), frame, with_hud);
        surface.image_snapshot()
    }
}

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

    /// Caméra de la première image (début de l'intro) et de la dernière (fin de l'outro).
    pub fn first_camera(&self) -> Camera {
        self.animation.camera_at(self.animation.moment_at(0))
    }

    pub fn last_camera(&self) -> Camera {
        self.animation.camera_at(self.animation.moment_at(self.frame_count().saturating_sub(1)))
    }

    /// Le fond de carte vu par une caméra libre (plan large d'un montage), avec le tracé complet ou sans.
    pub fn draw_view(&self, canvas: &Canvas, camera: Camera, with_route: bool) {
        let (width, height) = (self.input.video.width, self.input.video.height);
        let view = self.atlas.view(camera, width, height);
        self.atlas.draw(canvas, view, width, height);
        if with_route {
            self.route.draw(canvas, &view, Rect::from_wh(width as f32, height as f32), self.animation.total_meters());
        }
    }

    /// Les tuiles que lit `draw_view` pour cette caméra.
    pub fn tiles_at(&self, camera: Camera) -> BTreeSet<String> {
        let (width, height) = (self.input.video.width, self.input.video.height);
        self.atlas.chunks_for(camera, width, height).into_iter().flat_map(|chunk| self.atlas.tiles_of(chunk)).collect()
    }

    /// La carte d'infos seule de l'image `frame`, par-dessus ce qui est déjà dessiné.
    pub fn draw_hud(&self, canvas: &Canvas, frame: u32) {
        let moment = self.animation.moment_at(frame);
        let hud = self.animation.hud_at(self.animation.meters_of(moment));
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

impl Frames for Renderer {
    fn video(&self) -> &Video {
        &self.input.video
    }

    fn frame_count(&self) -> u32 {
        self.animation.timeline.frame_count()
    }

    fn needed_tiles(&self, frames: std::ops::Range<u32>) -> Vec<String> {
        let (width, height) = (self.input.video.width, self.input.video.height);
        let chunks: BTreeSet<_> = frames
            .flat_map(|frame| {
                self.atlas.chunks_for(self.animation.camera_at(self.animation.moment_at(frame)), width, height)
            })
            .collect();
        let tiles: BTreeSet<String> = chunks.into_iter().flat_map(|chunk| self.atlas.tiles_of(chunk)).collect();
        tiles.into_iter().collect()
    }

    fn draw(&self, canvas: &Canvas, frame: u32, with_hud: bool) {
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
            self.draw_hud(canvas, frame);
        }
    }
}

/// PNG ou JPEG d'après l'extension du fichier.
pub fn encode(image: &Image, file: &str) -> Option<Vec<u8>> {
    let format = if file.ends_with(".png") { EncodedImageFormat::PNG } else { EncodedImageFormat::JPEG };
    image.encode(None, format, 92).map(|data| data.as_bytes().to_vec())
}
