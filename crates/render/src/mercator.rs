//! Web Mercator en pixels « monde », avec des tuiles de 512 pixels comme MapLibre.

use crate::geo::LonLat;
use std::f64::consts::{FRAC_PI_4, PI};

pub fn world_size(zoom: f64) -> f64 {
    512.0 * zoom.exp2()
}

/// Pixel monde au zoom `zoom`, origine au coin nord-ouest.
pub fn project([lon, lat]: LonLat, zoom: f64) -> [f64; 2] {
    let size = world_size(zoom);
    let y = (FRAC_PI_4 + lat.to_radians() / 2.0).tan().ln();
    [(lon + 180.0) / 360.0 * size, (1.0 - y / PI) / 2.0 * size]
}

pub fn unproject([x, y]: [f64; 2], zoom: f64) -> LonLat {
    let size = world_size(zoom);
    let lon = x / size * 360.0 - 180.0;
    let lat = (2.0 * (PI * (1.0 - 2.0 * y / size)).exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees();
    [lon, lat]
}

/// Passage du monde à l'écran pour une image : niveau de l'atlas, échelle, coin haut gauche.
#[derive(Debug, Clone, Copy)]
pub struct View {
    /// Zoom du niveau de l'atlas dont les pixels monde servent de repère.
    pub zoom: f64,
    /// Pixels écran par pixel monde du niveau : 1 pendant le parcours, moins pendant l'intro et l'outro.
    pub scale: f64,
    /// Pixel monde (au zoom du niveau) du coin haut gauche de l'écran.
    pub origin: [f64; 2],
}

impl View {
    /// Centre de la caméra au milieu de l'écran. À l'échelle 1, coin calé sur un pixel entier : le fond
    /// est alors recopié tel quel, sans second filtrage qui le rendrait flou.
    pub fn new(center: LonLat, camera_zoom: f64, level_zoom: f64, width: u32, height: u32) -> Self {
        let mut scale = (camera_zoom - level_zoom).exp2();
        let [cx, cy] = project(center, level_zoom);
        let mut origin = [cx - f64::from(width) / 2.0 / scale, cy - f64::from(height) / 2.0 / scale];
        if (scale - 1.0).abs() < 1e-6 {
            scale = 1.0;
            origin = [origin[0].round(), origin[1].round()];
        }
        View { zoom: level_zoom, scale, origin }
    }

    pub fn to_screen(&self, point: LonLat) -> [f64; 2] {
        let [x, y] = project(point, self.zoom);
        [(x - self.origin[0]) * self.scale, (y - self.origin[1]) * self.scale]
    }

    /// Zoom effectif de l'image, pour les tailles qui en dépendent (`byZoom`).
    pub fn camera_zoom(&self) -> f64 {
        self.zoom + self.scale.log2()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_and_back() {
        let point = [2.35, 48.85];
        let [lon, lat] = unproject(project(point, 7.3), 7.3);
        assert!((lon - point[0]).abs() < 1e-9 && (lat - point[1]).abs() < 1e-9);
    }

    #[test]
    fn origin_is_north_west() {
        let [x, y] = project([0.0, 0.0], 0.0);
        assert!((x - 256.0).abs() < 1e-9 && (y - 256.0).abs() < 1e-9);
    }
}
