//! Une image de la vidéo : fond de carte, tracé, lieux, carte d'infos.

use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

/// Une image rendue, à encoder par `encode`.
pub use skia_safe::Image;
use skia_safe::{Canvas, EncodedImageFormat, Rect, Surface, surfaces};

use crate::atlas::Atlas;
use crate::color::parse_color;
use crate::frame::{Animation, Camera, Moment};
use crate::hud::{Hud, HudFrame, Profile};
use crate::light::Sun;
use crate::localities::Localities;
use crate::markers::Markers;
use crate::mercator::{Projection, Scaled};
use crate::model::{Input, Tour, Video};
use crate::route::Route;
use crate::sprites::Fonts;
use crate::stops::{Stops, StopsFrame};
use crate::terrain3d::Terrain3d;

/// Durée de l'effet « pop » d'un lieu atteint, en secondes de vidéo.
const POP_SECONDS: f64 = 0.6;
/// Part de l'outro au bout de laquelle les noms des lieux ont disparu.
const LABEL_FADE_PROGRESS: f64 = 0.5;
/// Points du profil d'altitude, et zoom du relief qu'il lit (≈ 40 m par pixel).
const PROFILE_SAMPLES: usize = 400;
const PROFILE_ZOOM: u8 = 11;
/// Taille du nom d'une localité traversée, en part de celle des repères : plus discret qu'eux.
const LOCALITY_LABEL_SHARE: f64 = 0.75;

/// Ce que la vidéo lit d'un rendu : un run seul (`Renderer`) ou plusieurs à la suite (`Montage`).
pub trait Frames: Sync {
    fn video(&self) -> &Video;
    fn frame_count(&self) -> u32;
    /// Tuiles à télécharger avant `needed_tiles`, qui les lit pour placer la caméra.
    fn camera_tiles(&self) -> Vec<String> {
        Vec::new()
    }
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
    /// Les noms des lieux s'effacent pendant le dézoom final ; un montage les garde jusqu'au vol.
    pub fade_labels_at_outro: bool,
    atlas: Arc<Atlas>,
    /// Le fond des transferts, s'il y en a un et des transferts.
    transfer_atlas: Option<Atlas>,
    /// La vue inclinée en relief (`camera.pitch`), si le GPU le permet.
    terrain: Option<Terrain3d>,
    /// Profil d'altitude de la carte d'infos (`hud.showProfile`), lu à la première image : ses tuiles
    /// sont téléchargées avant.
    profile: OnceLock<Option<Profile>>,
    /// Localités traversées (`stops.showLocalities`), lues à la première image : leurs tuiles sont
    /// téléchargées avant.
    localities: OnceLock<Option<Localities>>,
    route: Route,
    stops: Stops,
    markers: Markers,
    hud: Hud,
}

impl Renderer {
    /// `previous` : un rendu précédent (aperçu de l'éditeur) dont l'atlas est repris s'il ne dépend
    /// d'aucun réglage changé, avec ses morceaux déjà calculés.
    pub fn new(input: Input, tour: Arc<Tour>, previous: Option<&Renderer>) -> Self {
        let theme = &input.theme;
        let animation = Animation::new(tour.clone(), &input.video, &theme.camera);
        // Le fond se rend à la taille réelle : son zoom gagne l'échelle de l'image sur la référence.
        let travel_zoom = animation.travel_zoom() + input.video.scale().log2();
        // Sous le soleil, le relief s'éclaire sur le GPU : l'ombrage de l'atlas s'efface d'autant.
        let ground = {
            let mut ground = input.clone();
            if theme.camera.pitch > 0.0 {
                ground.theme.basemap.hillshade *= 1.0 - theme.camera.sunlight.clamp(0.0, 1.0);
            }
            ground
        };
        let atlas = match previous {
            Some(previous) if previous.atlas.serves(travel_zoom, &ground) => previous.atlas.clone(),
            _ => Arc::new(Atlas::new(travel_zoom, &ground)),
        };
        let transfer_atlas = input.transfer_imagery.clone().filter(|_| !tour.transfers.is_empty()).map(|imagery| {
            let mut transfer = ground.clone();
            transfer.imagery = imagery;
            transfer.sea_imagery = None;
            Atlas::new(travel_zoom, &transfer)
        });
        let (width, height) = (input.video.width, input.video.height);
        let terrain = (theme.camera.pitch > 0.0)
            .then(|| Terrain3d::new(&atlas, &theme.basemap, theme.camera.terrain_exaggeration, width, height))
            .and_then(|terrain| {
                terrain.map_err(|error| eprintln!("vue inclinée impossible, rendu à plat : {error}")).ok()
            });
        let fonts = Fonts::at_resolution(input.video.scale() as f32);
        let (reference_width, reference_height) = input.video.reference_size();
        // Chaque imagerie dessinée est créditée, une fois.
        let mut credits: Vec<&str> = Vec::new();
        for imagery in [Some(&input.imagery), input.sea_imagery.as_ref(), input.transfer_imagery.as_ref()] {
            if let Some(credit) = imagery.map(|imagery| imagery.credit.as_str()).filter(|c| !credits.contains(c)) {
                credits.push(credit);
            }
        }
        Renderer {
            route: Route::new(
                &tour.path,
                &tour.ferries,
                &tour.transfers,
                &tour
                    .stages
                    .iter()
                    .filter(|stage| stage.time_trial)
                    .map(|stage| [stage.from, stage.to])
                    .collect::<Vec<_>>(),
                &theme.route,
                fonts.emoji(&theme.route.head_icon),
            ),
            stops: Stops::new(&input, &tour, theme, &fonts),
            markers: Markers::new(
                &tour,
                theme,
                &fonts,
                |meters| {
                    let camera = animation.camera_at(Moment::Travel { meters });
                    atlas.view(camera, reference_width, reference_height)
                },
                reference_width as f32,
            ),
            hud: Hud::new(
                &theme.hud,
                &input.title,
                &credits.join(" · "),
                &input.hud_fonts,
                &tour.stops.iter().map(|stop| input.images.get(&stop.id).cloned()).collect::<Vec<_>>(),
                &input.video,
            ),
            atlas,
            transfer_atlas,
            terrain,
            profile: OnceLock::new(),
            localities: OnceLock::new(),
            animation,
            fade_labels_at_outro: true,
            tour,
            input,
        }
    }

    /// Caméra de la première image (début de l'intro) et de la dernière (fin de l'outro).
    pub fn first_camera(&self) -> Camera {
        self.hold_pitches();
        self.animation.camera_at(self.animation.moment_at(0))
    }

    pub fn last_camera(&self) -> Camera {
        self.hold_pitches();
        self.animation.camera_at(self.animation.moment_at(self.frame_count().saturating_sub(1)))
    }

    /// Le fond de carte vu par une caméra libre (plan large d'un montage), avec par-dessus le tracé complet
    /// et les lieux atteints, à l'opacité `finished` (0 : le fond seul).
    pub fn draw_view(&self, canvas: &Canvas, camera: Camera, finished: f32) {
        let (width, height) = (self.input.video.width, self.input.video.height);
        let view = self.atlas.view(self.on_screen(camera), width, height);
        self.atlas.draw(canvas, view, width, height);
        if finished <= 0.0 {
            return;
        }
        let total = self.animation.total_meters();
        canvas.save_layer_alpha_f(None, finished.min(1.0));
        let (view, screen) = self.to_reference(canvas, &view);
        self.route.draw(canvas, &view, screen, total);
        // Les lieux tels qu'à la dernière image du rendu.
        let stops_frame = StopsFrame {
            visited: self.tour.stops.len(),
            meters: total,
            pop_meters: self.pop_meters(),
            label_opacity: 1.0,
        };
        self.stops.draw(canvas, &view, screen, &stops_frame);
        canvas.restore();
        canvas.restore();
    }

    /// La caméra de l'image réelle pour une caméra de l'écran de référence : même sol vu, plus de pixels.
    fn on_screen(&self, camera: Camera) -> Camera {
        Camera { zoom: camera.zoom + self.input.video.scale().log2(), ..camera }
    }

    /// Passe `canvas` à l'écran de référence, jusqu'au prochain `restore` : la projection de l'image réelle
    /// qui s'y lit, et le rectangle de l'écran.
    fn to_reference<'a>(&self, canvas: &Canvas, view: &'a dyn Projection) -> (Scaled<'a>, Rect) {
        let scale = self.input.video.scale();
        let (width, height) = self.input.video.reference_size();
        canvas.save();
        canvas.scale((scale as f32, scale as f32));
        (Scaled { inner: view, scale }, Rect::from_wh(width as f32, height as f32))
    }

    /// Distance sur laquelle un lieu atteint « pope ».
    fn pop_meters(&self) -> f64 {
        self.animation.total_meters() / self.input.video.duration_sec * POP_SECONDS
    }

    /// Les tuiles que lit `draw_view` pour cette caméra.
    pub fn tiles_at(&self, camera: Camera) -> BTreeSet<String> {
        let (width, height) = (self.input.video.width, self.input.video.height);
        let camera = self.on_screen(camera);
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
            profile: self.profile().map(|profile| {
                let total = self.animation.total_meters();
                (profile, if total > 0.0 { self.animation.meters_of(moment) / total } else { 0.0 })
            }),
        };
        let scale = self.input.video.scale() as f32;
        canvas.save();
        canvas.scale((scale, scale));
        self.hud.draw(canvas, &hud_frame);
        canvas.restore();
    }

    /// Points du tracé où se lit le profil d'altitude.
    fn profile_points(&self) -> Vec<crate::geo::LonLat> {
        let total = self.tour.path.total_meters();
        (0..PROFILE_SAMPLES).map(|i| self.tour.path.point_at(total * i as f64 / (PROFILE_SAMPLES - 1) as f64)).collect()
    }

    fn profile(&self) -> Option<&Profile> {
        self.profile
            .get_or_init(|| {
                self.input.theme.hud.show_profile.then(|| {
                    let elevation = self.atlas.elevation();
                    let zoom = PROFILE_ZOOM.min(elevation.maxzoom);
                    let altitudes =
                        self.profile_points().iter().map(|&p| elevation.altitude_at(p, zoom) as f32).collect();
                    Profile { altitudes }
                })
            })
            .as_ref()
    }

    fn localities(&self) -> Option<&Localities> {
        self.localities
            .get_or_init(|| {
                let theme = &self.input.theme;
                theme.stops.show_localities.then(|| {
                    let colors = [parse_color(&theme.hud.text_color), parse_color(&theme.hud.background)];
                    let size = (theme.stops.marker_label_size * LOCALITY_LABEL_SHARE) as f32;
                    let screen = self.animation.screen_meters();
                    let (dir, source) = (&self.input.tile_dir, &self.input.vector.name);
                    let fonts = Fonts::at_resolution(self.input.video.scale() as f32);
                    Localities::new(dir, source, &self.tour, &fonts, size, colors, screen)
                })
            })
            .as_ref()
    }

    /// En relief, redresse la caméra là où le relief cacherait la tête (`sight`), une fois ses tuiles
    /// (`camera_tiles`) téléchargées.
    fn hold_pitches(&self) {
        if self.terrain.is_some() {
            // La géométrie de la vue inclinée ne dépend que du format : l'écran de référence suffit.
            let (width, height) = self.input.video.reference_size();
            let size = [width, height];
            let exaggeration = self.input.theme.camera.terrain_exaggeration;
            let elevation = self.atlas.elevation();
            self.animation.hold_pitches(|| crate::sight::limits(&self.animation, &elevation, size, exaggeration));
        }
    }

    /// Tuiles des localités traversées, si elles sont affichées.
    fn locality_tiles(&self) -> BTreeSet<String> {
        if !self.input.theme.stops.show_localities {
            return BTreeSet::new();
        }
        crate::localities::tiles(&self.tour.path, &self.input.vector.name)
    }

    /// Tuiles de relief du profil d'altitude, s'il est affiché.
    fn profile_tiles(&self) -> BTreeSet<String> {
        if !self.input.theme.hud.show_profile {
            return BTreeSet::new();
        }
        let elevation = self.atlas.elevation();
        let zoom = PROFILE_ZOOM.min(elevation.maxzoom);
        let tiles: Vec<_> = self.profile_points().iter().flat_map(|&point| elevation.tiles_at(point, zoom)).collect();
        elevation.tile_names(&tiles)
    }
}

impl Frames for Renderer {
    fn video(&self) -> &Video {
        &self.input.video
    }

    fn frame_count(&self) -> u32 {
        self.animation.timeline.frame_count()
    }

    fn camera_tiles(&self) -> Vec<String> {
        if self.terrain.is_none() {
            return Vec::new();
        }
        // La géométrie de la vue inclinée ne dépend que du format : l'écran de référence suffit.
        let (width, height) = self.input.video.reference_size();
        let size = [width, height];
        let exaggeration = self.input.theme.camera.terrain_exaggeration;
        crate::sight::tiles(&self.animation, &self.atlas.elevation(), size, exaggeration).into_iter().collect()
    }

    fn needed_tiles(&self, frames: std::ops::Range<u32>) -> Vec<String> {
        let (width, height) = (self.input.video.width, self.input.video.height);
        if let Some(terrain) = &self.terrain {
            self.hold_pitches();
            let mut tiles = self.needed_tiles_3d(terrain, frames);
            tiles.extend(self.profile_tiles());
            tiles.extend(self.locality_tiles());
            tiles.sort();
            tiles.dedup();
            return tiles;
        }
        let mut chunks = BTreeSet::new();
        let mut transfer_chunks = BTreeSet::new();
        for frame in frames {
            let moment = self.animation.moment_at(frame);
            let camera = self.on_screen(self.animation.camera_at(moment));
            chunks.extend(self.atlas.chunks_for(camera, width, height));
            // Le fond des transferts, seulement là où il se voit.
            if let Some(transfer) = &self.transfer_atlas
                && self.animation.transfer_blend(self.animation.meters_of(moment)) > 0.0
            {
                transfer_chunks.extend(transfer.chunks_for(camera, width, height));
            }
        }
        let mut tiles: BTreeSet<String> = chunks.into_iter().flat_map(|chunk| self.atlas.tiles_of(chunk)).collect();
        if let Some(transfer) = &self.transfer_atlas {
            tiles.extend(transfer_chunks.into_iter().flat_map(|chunk| transfer.tiles_of(chunk)));
        }
        tiles.extend(self.profile_tiles());
        tiles.extend(self.locality_tiles());
        tiles.into_iter().collect()
    }

    fn draw(&self, canvas: &Canvas, frame: u32, with_hud: bool) {
        let (width, height) = (self.input.video.width, self.input.video.height);
        let moment = self.animation.moment_at(frame);
        let camera = self.on_screen(self.animation.camera_at(moment));
        let meters = self.animation.meters_of(moment);
        let blend = self.animation.transfer_blend(meters);
        if let Some(terrain) = &self.terrain {
            self.hold_pitches();
            let perspective = terrain.perspective(camera, width, height);
            let transfer = self.transfer_atlas.as_ref().map(|transfer| (transfer, blend));
            let sun = self.sun_at(frame, meters, camera.center);
            let rendered = terrain.render(&self.atlas, transfer, &perspective, sun, canvas);
            let lift = self.input.theme.route.width / 2.0 * self.input.video.scale();
            let projection = terrain.projection(&self.atlas, &perspective, &rendered.depth, rendered.depth_size, lift);
            self.draw_overlays(canvas, &projection, frame, with_hud);
            return;
        }
        let view = self.atlas.view(camera, width, height);
        self.atlas.draw(canvas, view, width, height);
        if let Some(transfer) = &self.transfer_atlas
            && blend > 0.0
        {
            canvas.save_layer_alpha_f(None, blend as f32);
            transfer.draw(canvas, transfer.view(camera, width, height), width, height);
            canvas.restore();
        }
        self.draw_overlays(canvas, &view, frame, with_hud);
    }
}

impl Renderer {
    /// Le soleil de l'image, si le relief est éclairé : celui de l'heure atteinte au point visé quand le
    /// parcours a des horaires (l'heure suit les mètres parcourus), sinon sur sa course stylisée de
    /// `camera.sunFrom` à `camera.sunTo` au fil de la vidéo.
    fn sun_at(&self, frame: u32, meters: f64, center: [f64; 2]) -> Option<Sun> {
        let light = &self.input.theme.camera;
        if light.sunlight <= 0.0 {
            return None;
        }
        if let Some(clock) = &self.input.clock {
            return Some(Sun::at_time(clock.at(meters), center, light.sunlight));
        }
        let progress = f64::from(frame) / f64::from(self.frame_count().saturating_sub(1).max(1));
        let course = light.sun_from + (light.sun_to - light.sun_from) * progress;
        Some(Sun::at(course, center[1], light.sunlight))
    }

    /// Tracé, tête, repères, lieux et carte d'infos de l'image, par-dessus le fond.
    fn draw_overlays(&self, canvas: &Canvas, view: &dyn Projection, frame: u32, with_hud: bool) {
        let (view, screen) = self.to_reference(canvas, view);
        let view = &view;
        let moment = self.animation.moment_at(frame);
        let meters = self.animation.meters_of(moment);
        let hud = self.animation.hud_at(meters);
        self.route.draw(canvas, view, screen, meters);
        // Ni avant le départ, ni sur la vue d'ensemble finale, ni entre deux étapes.
        if matches!(moment, Moment::Travel { .. }) && !self.tour.in_transfer(meters) {
            self.route.draw_head(canvas, view, meters);
        }
        let pop_meters = self.pop_meters();
        let label_opacity = match moment {
            Moment::Outro { progress } if self.fade_labels_at_outro => (1.0 - progress / LABEL_FADE_PROGRESS).max(0.0),
            _ => 1.0,
        };
        if let Some(localities) = self.localities().filter(|_| matches!(moment, Moment::Travel { .. })) {
            localities.draw(canvas, view, screen, meters, label_opacity as f32);
        }
        self.markers.draw(canvas, view, screen, meters, pop_meters, label_opacity as f32);
        let stops_frame = StopsFrame { visited: hud.visited, meters, pop_meters, label_opacity: label_opacity as f32 };
        self.stops.draw(canvas, view, screen, &stops_frame);
        canvas.restore();
        if with_hud {
            self.draw_hud(canvas, frame);
        }
    }

    /// Les tuiles des morceaux en relief que voient les images `frames`, et de leurs maillages.
    fn needed_tiles_3d(&self, terrain: &Terrain3d, frames: std::ops::Range<u32>) -> Vec<String> {
        let (width, height) = (self.input.video.width, self.input.video.height);
        let exaggeration = self.input.theme.camera.terrain_exaggeration;
        let mut chunks = BTreeSet::new();
        let mut transfer_chunks = BTreeSet::new();
        let mut casters = BTreeSet::new();
        let sunlit = self.input.theme.camera.sunlight > 0.0;
        // L'altitude du point visé, comme au dessin : sous un point visé haut, la vue plonge plus loin dans les
        // vallées. Ses tuiles sont déjà là (`camera_tiles`) ; lue à part, une tuile encore absente n'est pas
        // retenue comme manquante par le relief du dessin.
        let elevation = self.atlas.elevation().detached();
        for frame in frames {
            let moment = self.animation.moment_at(frame);
            let camera = self.on_screen(self.animation.camera_at(moment));
            let ground = crate::sight::ground(&elevation, camera, [width, height]);
            let perspective = crate::perspective::Perspective::new(camera, width, height, ground, exaggeration);
            let seen = terrain.chunks(&self.atlas, &perspective);
            if sunlit {
                casters.extend(terrain.shadow_casters(&self.atlas, &perspective));
            }
            if self.transfer_atlas.is_some() && self.animation.transfer_blend(self.animation.meters_of(moment)) > 0.0 {
                transfer_chunks.extend(seen.iter().copied());
            }
            chunks.extend(seen);
        }
        let chunks: Vec<_> = chunks.into_iter().collect();
        let mut tiles = terrain.tiles_of(&self.atlas, &chunks);
        tiles.extend(terrain.caster_tiles(&casters.into_iter().collect::<Vec<_>>()));
        if let Some(transfer) = &self.transfer_atlas {
            let transfer_chunks: Vec<_> = transfer_chunks.into_iter().collect();
            tiles.extend(transfer_chunks.iter().flat_map(|&key| transfer.tiles_of(key)));
        }
        tiles.into_iter().collect()
    }
}

/// PNG ou JPEG d'après l'extension du fichier.
pub fn encode(image: &Image, file: &str) -> Option<Vec<u8>> {
    let format = if file.ends_with(".png") { EncodedImageFormat::PNG } else { EncodedImageFormat::JPEG };
    image.encode(None, format, 92).map(|data| data.as_bytes().to_vec())
}
