//! Une scène : ce que le rendu d'un run lit hors du thème (tournée, titre, images des lieux, cache).
//! Lue une fois, elle se rend dans plusieurs thèmes (éditeur) et sert la vidéo comme la couverture.

use std::collections::HashMap;
use std::sync::Arc;

use dijkstravel_core::places::Place;
use dijkstravel_core::run::Run;
use dijkstravel_core::theme::Theme;
use dijkstravel_core::tour::Tour;
use skia_safe::Image;

use crate::download::{self, Downloader};
use crate::frame::Timeline;
use crate::model::{Input, Video};
use crate::monitor::Monitor;
use crate::render::{Frames, Renderer};
use crate::video;

pub struct Scene {
    pub run: Run,
    pub tour: Arc<Tour>,
    title: String,
    images: HashMap<String, String>,
    /// Lieux dont l'image est introuvable : identifiant, adresse. Ils se dessinent sans.
    pub missing_images: Vec<(String, String)>,
    downloader: Downloader,
}

impl Scene {
    /// Télécharge au besoin les images des lieux dans le cache de `downloader`.
    pub fn new(run: Run, tour: Tour, places: &[Place], downloader: Downloader) -> Scene {
        let images = downloader.place_images(places);
        let title = run.title.clone().unwrap_or_else(|| format!("{} lieux · {}", tour.stops.len(), run.name));
        Scene { run, tour: Arc::new(tour), title, images: images.files, missing_images: images.missing, downloader }
    }

    pub fn video(&self) -> Video {
        let video = &self.run.video;
        let (width, height) = video.format.size();
        Video {
            width,
            height,
            duration_sec: video.duration_sec,
            intro_sec: video.intro_sec,
            outro_sec: video.outro_sec,
            fps: video.fps,
        }
    }

    pub fn frame_count(&self) -> u32 {
        Timeline::new(&self.video()).frame_count()
    }

    pub fn input(&self, theme: Theme) -> Result<Input, String> {
        Ok(Input {
            video: self.video(),
            title: self.title.clone(),
            images: self.images.clone(),
            imagery: download::imagery_source(&theme.basemap.imagery),
            sea_imagery: Some(theme.basemap.sea_imagery.as_str())
                .filter(|name| !name.is_empty())
                .map(download::imagery_source),
            transfer_imagery: Some(theme.basemap.transfer_imagery.as_str())
                .filter(|name| !name.is_empty())
                .map(download::imagery_source),
            terrain: download::terrain_source(),
            vector: download::vector_source(),
            tile_dir: self.downloader.tile_dir().display().to_string(),
            hud_fonts: self.downloader.hud_fonts(&theme.hud.font)?,
            theme,
        })
    }

    /// `previous` : un rendu d'un autre thème dont l'atlas est repris s'il reste valable.
    pub fn renderer(&self, theme: Theme, previous: Option<&Renderer>) -> Result<Renderer, String> {
        Ok(Renderer::new(self.input(theme)?, self.tour.clone(), previous))
    }

    /// Télécharge dans le cache de la scène les tuiles que liront les images `frames` d'un rendu, celui
    /// de la scène ou un montage dont elle fait partie.
    pub fn prefetch(
        &self,
        renderer: &dyn Frames,
        frames: std::ops::Range<u32>,
        monitor: &dyn Monitor,
    ) -> Result<(), String> {
        self.downloader.fetch_tiles(&renderer.needed_tiles(frames), monitor)
    }

    /// L'image `frame` (la dernière au-delà), tuiles téléchargées avant ; `with_hud` : avec la carte d'infos.
    pub fn still(
        &self,
        renderer: &dyn Frames,
        frame: u32,
        with_hud: bool,
        monitor: &dyn Monitor,
    ) -> Result<Image, String> {
        self.prefetch(renderer, frame..frame + 1, monitor)?;
        Ok(renderer.render(frame.min(renderer.frame_count().saturating_sub(1)), with_hud))
    }

    /// La couverture : toute dernière image, fin de l'outro, tracé complet dézoomé, sans la carte
    /// d'infos (place libre pour un titre).
    pub fn cover(&self, renderer: &dyn Frames, monitor: &dyn Monitor) -> Result<Image, String> {
        self.still(renderer, renderer.frame_count().saturating_sub(1), false, monitor)
    }

    /// Images de la vidéo entière, ou de ses `seconds` premières secondes.
    pub fn frames_in(&self, renderer: &dyn Frames, seconds: Option<f64>) -> u32 {
        let all = renderer.frame_count();
        seconds.map_or(all, |seconds| ((seconds * f64::from(self.run.video.fps)).round() as u32).min(all))
    }

    /// La vidéo (ou ses `seconds` premières secondes) dans `out`, tuiles téléchargées avant.
    pub fn encode_video(
        &self,
        renderer: &dyn Frames,
        seconds: Option<f64>,
        threads: usize,
        out: &str,
        monitor: &dyn Monitor,
    ) -> Result<(), String> {
        let frames = self.frames_in(renderer, seconds);
        self.prefetch(renderer, 0..frames, monitor)?;
        video::encode_video(renderer, frames, threads, out, monitor)
    }
}
