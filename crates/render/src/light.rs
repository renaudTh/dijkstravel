//! Le soleil sur la vue inclinée (`camera.sunlight`) : il suit sa vraie course du jour à la latitude du
//! parcours, de `camera.sunFrom` à `camera.sunTo` au fil de la vidéo. Sa lumière traverse d'autant plus
//! d'atmosphère qu'il est bas et rougit (extinction par la masse d'air), le ciel bleuté éclaire les
//! pentes à l'ombre, la brume et le ciel se teintent vers lui.

use crate::gpu::Light;

/// Déclinaison du soleil, en degrés : celle de la fin août ou de la mi-avril, entre l'équinoxe et le
/// solstice d'été (le run n'a pas de date).
const DECLINATION: f64 = 12.0;
/// Le soleil ne descend pas plus bas pour éclairer : à son lever ou à son coucher, sa lumière resterait
/// noire.
const MIN_ELEVATION: f64 = 1.0;
/// Épaisseur optique de l'atmosphère au zénith, par canal (rouge, vert, bleu) : diffusion de Rayleigh
/// et brume, le bleu s'éteint le premier.
const EXTINCTION: [f64; 3] = [0.10, 0.24, 0.52];
/// Ciel qui éclaire les pentes à l'ombre, en plein jour et au crépuscule.
const DAY_SKY: [f64; 3] = [0.34, 0.40, 0.52];
const TWILIGHT_SKY: [f64; 3] = [0.30, 0.27, 0.42];
/// Lumière directe du soleil, face à lui, au regard de celle du ciel : des versants contrastés.
const SUN_GAIN: f64 = 2.2;
/// Brume et haut du ciel d'un soleil rasant.
const SUNSET_HAZE: [f64; 3] = [1.0, 0.64, 0.42];
const SUNSET_SKY: [f64; 3] = [0.30, 0.36, 0.64];
/// Hauteur de référence : éclairé par un soleil à cette hauteur, le relief garde ses couleurs.
const REFERENCE_ELEVATION: f64 = 40.0;
/// Part de la baisse de lumière que l'exposition rattrape : 0, le relief s'assombrit comme en vrai ; 1, il
/// garde sa luminosité moyenne.
const EXPOSURE_COMPENSATION: f64 = 0.75;

#[derive(Debug, Clone, Copy)]
pub struct Sun {
    /// Hauteur au-dessus de l'horizon, en degrés.
    pub elevation: f64,
    /// Direction, en degrés depuis le nord vers l'est.
    pub azimuth: f64,
    /// `camera.sunset`.
    pub strength: f64,
}

impl Sun {
    /// Le soleil à la position `course` de sa course du jour (degrés : 0 à son lever, 90 au plus haut,
    /// 180 à son coucher), vu à la latitude `latitude`.
    pub fn at(course: f64, latitude: f64, strength: f64) -> Sun {
        // Latitude bornée : aux pôles, ni lever ni coucher.
        let (phi, delta) = (latitude.clamp(-66.0, 66.0).to_radians(), DECLINATION.to_radians());
        // Angle horaire du coucher ; la course le parcourt depuis celui du lever, son opposé.
        let sunset_hour = (-phi.tan() * delta.tan()).clamp(-1.0, 1.0).acos();
        let hour = sunset_hour * (course.clamp(0.0, 180.0) / 90.0 - 1.0);
        let sin_elevation = phi.sin() * delta.sin() + phi.cos() * delta.cos() * hour.cos();
        let elevation = sin_elevation.clamp(-1.0, 1.0).asin();
        // Depuis le nord vers l'est : à l'est le matin (angle horaire négatif), au sud à midi.
        let azimuth = (-delta.cos() * hour.sin()).atan2((delta.sin() - sin_elevation * phi.sin()) / phi.cos());
        Sun {
            elevation: elevation.to_degrees().max(MIN_ELEVATION),
            azimuth: azimuth.to_degrees().rem_euclid(360.0),
            strength,
        }
    }

    /// Vers le soleil, en repère local (x à l'est, y au nord, z en haut).
    fn direction(&self) -> [f64; 3] {
        let (elevation, azimuth) = (self.elevation.to_radians(), self.azimuth.to_radians());
        [elevation.cos() * azimuth.sin(), elevation.cos() * azimuth.cos(), elevation.sin()]
    }

    /// Couleur de la lumière directe, blanche au zénith.
    fn color(elevation: f64) -> [f64; 3] {
        // Masse d'air de Kasten et Young, relative au zénith.
        let air_mass = 1.0 / (elevation.to_radians().sin() + 0.50572 * (elevation + 6.07995).powf(-1.6364));
        EXTINCTION.map(|tau| SUN_GAIN * (-tau * (air_mass - 1.0)).exp())
    }

    fn ambient(elevation: f64) -> [f64; 3] {
        let day = smoothstep(0.0, 20.0, elevation);
        std::array::from_fn(|i| mix(TWILIGHT_SKY[i], DAY_SKY[i], day))
    }

    /// Lumière d'un sol plat : soleil (selon sa hauteur) et ciel.
    fn flat_light(elevation: f64) -> f64 {
        luminance(Self::color(elevation)) * elevation.to_radians().sin() + luminance(Self::ambient(elevation))
    }

    pub fn light(&self) -> Light {
        let elevation = self.elevation.max(0.0);
        // Un sol plat sous le soleil de référence garde ses couleurs ; plus bas, il n'en rattrape qu'une part.
        let reference = Self::flat_light(REFERENCE_ELEVATION);
        let exposure = (reference / Self::flat_light(elevation)).powf(EXPOSURE_COMPENSATION) / reference;
        Light {
            direction: self.direction().map(|v| v as f32),
            strength: self.strength.clamp(0.0, 1.0) as f32,
            sun: Self::color(elevation).map(|v| v as f32),
            ambient: Self::ambient(elevation).map(|v| v as f32),
            exposure: exposure as f32,
        }
    }

    /// Brume et haut du ciel (RVBA de 0 à 1) teintés par le soleil, d'autant plus qu'il est bas.
    pub fn atmosphere(&self, haze: [f32; 4], sky: [f32; 4]) -> ([f32; 4], [f32; 4]) {
        let warmth = (1.0 - smoothstep(0.0, 25.0, self.elevation)) * self.strength.clamp(0.0, 1.0);
        let tinted = |color: [f32; 4], target: [f64; 3], share: f64| {
            let [r, g, b] = std::array::from_fn(|i| mix(f64::from(color[i]), target[i], share) as f32);
            [r, g, b, color[3]]
        };
        (tinted(haze, SUNSET_HAZE, warmth * 0.85), tinted(sky, SUNSET_SKY, warmth * 0.7))
    }
}

fn mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn smoothstep(from: f64, to: f64, x: f64) -> f64 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn luminance([r, g, b]: [f64; 3]) -> f64 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_its_course_from_east_to_south_to_west() {
        let at = |course| Sun::at(course, 45.0, 1.0);
        // Lever à l'est-nord-est, midi plein sud, coucher à l'ouest-nord-ouest.
        assert!(at(1.0).azimuth > 60.0 && at(1.0).azimuth < 90.0 && at(1.0).elevation < 2.0, "{:?}", at(1.0));
        assert!((at(90.0).azimuth - 180.0).abs() < 1e-6, "{:?}", at(90.0));
        assert!((at(90.0).elevation - 57.0).abs() < 1e-6, "{:?}", at(90.0));
        assert!(at(179.0).azimuth > 270.0 && at(179.0).azimuth < 300.0, "{:?}", at(179.0));
        // Le matin et l'après-midi symétriques autour de midi.
        assert!((at(40.0).elevation - at(140.0).elevation).abs() < 1e-9);
        assert!((at(40.0).azimuth + at(140.0).azimuth - 360.0).abs() < 1e-9);
    }

    #[test]
    fn the_low_sun_turns_red_and_the_light_stays_readable() {
        let high = Sun { elevation: 20.0, azimuth: 255.0, strength: 1.0 }.light();
        let low = Sun { elevation: 2.0, azimuth: 255.0, strength: 1.0 }.light();
        // Haut, doré ; rasant, rouge orangé.
        assert!(high.sun[2] / high.sun[0] > 0.35, "{:?}", high.sun);
        assert!(low.sun[2] / low.sun[0] < 0.05 && low.sun[1] / low.sun[0] < 0.4, "{:?}", low.sun);
        // L'exposition remonte la lumière du soleil couchant.
        assert!(low.exposure > high.exposure);
        // Vers l'ouest-sud-ouest, un peu au-dessus de l'horizon.
        assert!(low.direction[0] < -0.9 && low.direction[1] < 0.0 && low.direction[2] > 0.0);
    }
}
