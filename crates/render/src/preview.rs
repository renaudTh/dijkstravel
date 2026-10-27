//! Aperçu d'un thème en cours de réglage : une image, rendue comme la vidéo la rendra.

use dijkstravel_core::theme::Theme;
use skia_safe::Image;

use crate::monitor::Silent;
use crate::render::Renderer;
use crate::scene::Scene;

pub struct Previewer {
    pub scene: Scene,
    /// Le dernier rendu, gardé tant que le thème ne change pas ; sinon son atlas resservira au suivant.
    current: Option<(Theme, Renderer)>,
}

impl Previewer {
    pub fn new(scene: Scene) -> Self {
        Previewer { scene, current: None }
    }

    /// L'image `frame` dans `theme`, avec la carte d'infos.
    pub fn frame(&mut self, theme: &Theme, frame: u32) -> Result<Image, String> {
        if self.current.as_ref().is_none_or(|(known, _)| known != theme) {
            let previous = self.current.take().map(|(_, renderer)| renderer);
            let renderer = self.scene.renderer(theme.clone(), previous.as_ref())?;
            self.current = Some((theme.clone(), renderer));
        }
        let (_, renderer) = self.current.as_ref().expect("rendu chargé");
        self.scene.still(renderer, frame, true, &Silent)
    }
}
