//! Le soleil sur la vue inclinée (`camera.sunlight`) : il suit sa vraie course du jour et de la nuit à la
//! latitude du parcours, de `camera.sunFrom` à `camera.sunTo` au fil de la vidéo (0 : lever, 90 : midi,
//! 180 : coucher, 270 : minuit, 360 : lever suivant). Sa lumière traverse d'autant plus d'atmosphère qu'il
//! est bas et rougit (extinction par la masse d'air), le ciel bleuté éclaire les pentes à l'ombre, la brume
//! et le ciel se teintent vers lui.
//!
//! Sous l'horizon, sa lueur s'attarde du côté où il s'est couché, puis le ciel passe à l'heure bleue et à
//! la nuit. La pleine lune, à l'opposé du soleil, se lève quand il se couche : sa lumière froide éclaire
//! le relief et porte ses ombres, les couleurs pâlissent et bleuissent, des étoiles s'allument dans le haut
//! du ciel. Soleil et
//! lune ne brillent jamais ensemble : la lumière passe de l'un à l'autre quand ni l'un ni l'autre
//! n'éclaire, sans saut d'ombre.

use crate::gpu::Light;

/// Déclinaison du soleil, en degrés : celle de la fin août ou de la mi-avril, entre l'équinoxe et le
/// solstice d'été (le run n'a pas de date). La pleine lune, à l'opposé, a la déclinaison contraire.
const DECLINATION: f64 = 12.0;
/// Le soleil n'éclaire pas sous cette hauteur : à son lever ou à son coucher, sa lumière resterait noire.
const MIN_ELEVATION: f64 = 1.0;
/// La lumière directe du soleil s'éteint de cette hauteur à `MIN_ELEVATION`, sous l'horizon : il se couche.
const SUNSET_ELEVATION: f64 = -3.0;
/// Hauteur du soleil, sous l'horizon, où la lumière passe de lui à la lune : aucun des deux n'y éclaire.
const SWITCH_ELEVATION: f64 = -3.5;
/// La lune éclaire à partir de cette hauteur du soleil et en plein à la seconde : la nuit tombe.
const MOONRISE: [f64; 2] = [-4.0, -12.0];
/// La lueur du couchant s'attarde jusqu'à cette hauteur du soleil.
const AFTERGLOW_ELEVATION: f64 = -8.0;
/// Hauteurs du soleil de l'heure bleue et de la nuit noire.
const BLUE_HOUR_ELEVATION: f64 = -5.0;
const NIGHT_ELEVATION: f64 = -14.0;
/// Épaisseur optique de l'atmosphère au zénith, par canal (rouge, vert, bleu) : diffusion de Rayleigh
/// et brume, le bleu s'éteint le premier.
const EXTINCTION: [f64; 3] = [0.10, 0.24, 0.52];
/// Ciel qui éclaire les pentes à l'ombre, en plein jour, au crépuscule et la nuit.
const DAY_SKY: [f64; 3] = [0.34, 0.40, 0.52];
const TWILIGHT_SKY: [f64; 3] = [0.30, 0.27, 0.42];
const NIGHT_SKY: [f64; 3] = [0.010, 0.013, 0.026];
/// Lumière directe du soleil, face à lui, au regard de celle du ciel : des versants contrastés.
const SUN_GAIN: f64 = 2.2;
/// Lumière de la pleine lune haute, bleutée : assez forte devant le ciel nocturne pour porter des ombres.
const MOON: [f64; 3] = [0.022, 0.028, 0.042];
/// Brume et haut du ciel d'un soleil rasant, de l'heure bleue et de la nuit.
const SUNSET_HAZE: [f64; 3] = [1.0, 0.64, 0.42];
const SUNSET_SKY: [f64; 3] = [0.30, 0.36, 0.64];
const BLUE_HOUR_HAZE: [f64; 3] = [0.36, 0.38, 0.54];
const BLUE_HOUR_SKY: [f64; 3] = [0.12, 0.17, 0.38];
const NIGHT_HAZE: [f64; 3] = [0.075, 0.09, 0.14];
const NIGHT_SKY_COLOR: [f64; 3] = [0.012, 0.02, 0.055];
/// Hauteur de référence : éclairé par un soleil à cette hauteur, le relief garde ses couleurs.
const REFERENCE_ELEVATION: f64 = 40.0;
/// Part de la baisse de lumière que l'exposition rattrape, de jour et de nuit : 0, le relief s'assombrit
/// comme en vrai ; 1, il garde sa luminosité moyenne. La nuit, il reste au tiers environ de celle du
/// jour.
const EXPOSURE_COMPENSATION: [f64; 2] = [0.75, 0.73];

#[derive(Debug, Clone, Copy)]
pub struct Sun {
    /// Hauteur au-dessus de l'horizon, en degrés, négative la nuit.
    pub elevation: f64,
    /// Direction, en degrés depuis le nord vers l'est.
    pub azimuth: f64,
    /// La pleine lune, à l'opposé du soleil.
    pub moon_elevation: f64,
    pub moon_azimuth: f64,
    /// `camera.sunlight`.
    pub strength: f64,
}

impl Sun {
    /// Le soleil à la position `course` de sa course (degrés : 0 à son lever, 90 au plus haut, 180 à son
    /// coucher, 270 au plus bas, 360 au lever suivant), vu à la latitude `latitude`.
    pub fn at(course: f64, latitude: f64, strength: f64) -> Sun {
        // Latitude bornée : aux pôles, ni lever ni coucher.
        let phi = latitude.clamp(-66.0, 66.0).to_radians();
        let delta = DECLINATION.to_radians();
        // Angle horaire du coucher. Le jour parcourt de son opposé (le lever) à lui, la nuit de lui au
        // lever du lendemain, en passant par minuit.
        let sunset_hour = (-phi.tan() * delta.tan()).clamp(-1.0, 1.0).acos();
        let course = course.clamp(0.0, 360.0);
        let hour = if course <= 180.0 {
            sunset_hour * (course / 90.0 - 1.0)
        } else {
            sunset_hour + (std::f64::consts::TAU - 2.0 * sunset_hour) * (course - 180.0) / 180.0
        };
        let (elevation, azimuth) = position(hour, delta, phi);
        let (moon_elevation, moon_azimuth) = position(hour - std::f64::consts::PI, -delta, phi);
        Sun { elevation, azimuth, moon_elevation, moon_azimuth, strength }
    }

    /// Couleur de la lumière directe, blanche au zénith.
    fn color(elevation: f64) -> [f64; 3] {
        // Masse d'air de Kasten et Young, relative au zénith.
        let air_mass = 1.0 / (elevation.to_radians().sin() + 0.50572 * (elevation + 6.07995).powf(-1.6364));
        EXTINCTION.map(|tau| SUN_GAIN * (-tau * (air_mass - 1.0)).exp())
    }

    /// Lumière du ciel : du jour au crépuscule au-dessus de l'horizon, du crépuscule à la nuit dessous.
    fn ambient(elevation: f64) -> [f64; 3] {
        if elevation >= 0.0 {
            mix3(TWILIGHT_SKY, DAY_SKY, smoothstep(0.0, 20.0, elevation))
        } else {
            mix3(TWILIGHT_SKY, NIGHT_SKY, smoothstep(0.0, MOONRISE[1], elevation))
        }
    }

    /// Lumière directe du soleil, éteinte une fois couché.
    fn sunlight(elevation: f64) -> [f64; 3] {
        let up = smoothstep(SUNSET_ELEVATION, MIN_ELEVATION, elevation);
        Self::color(elevation.max(MIN_ELEVATION)).map(|c| c * up)
    }

    /// Lumière directe de la lune, à la nuit tombée et quand elle est levée.
    fn moonlight(&self) -> [f64; 3] {
        let night = smoothstep(MOONRISE[0], MOONRISE[1], self.elevation);
        let risen = smoothstep(0.0, 6.0, self.moon_elevation);
        MOON.map(|c| c * night * risen)
    }

    /// Lumière d'un sol plat : soleil et lune (selon leur hauteur) et ciel.
    fn flat_light(&self) -> f64 {
        let sun = luminance(Self::sunlight(self.elevation)) * self.elevation.max(MIN_ELEVATION).to_radians().sin();
        let moon = luminance(self.moonlight()) * self.moon_elevation.max(MIN_ELEVATION).to_radians().sin();
        sun + moon + luminance(Self::ambient(self.elevation))
    }

    pub fn light(&self) -> Light {
        // Un sol plat sous le soleil de référence garde ses couleurs ; plus bas, il n'en rattrape qu'une part.
        let reference = Sun { elevation: REFERENCE_ELEVATION, moon_elevation: -REFERENCE_ELEVATION, ..*self };
        let reference = reference.flat_light();
        let night = smoothstep(MOONRISE[0], MOONRISE[1], self.elevation);
        let compensation = mix(EXPOSURE_COMPENSATION[0], EXPOSURE_COMPENSATION[1], night);
        let exposure = (reference / self.flat_light()).powf(compensation) / reference;
        // Une seule lumière directe : le soleil, puis la lune passé `SWITCH_ELEVATION`.
        let (toward, direct) = if self.elevation > SWITCH_ELEVATION {
            (direction(self.elevation.max(MIN_ELEVATION), self.azimuth), Self::sunlight(self.elevation))
        } else {
            (direction(self.moon_elevation.max(MIN_ELEVATION), self.moon_azimuth), self.moonlight())
        };
        let afterglow = smoothstep(AFTERGLOW_ELEVATION, 0.0, self.elevation);
        let stars = smoothstep(-6.0, NIGHT_ELEVATION, self.elevation);
        let strength = self.strength.clamp(0.0, 1.0);
        let f32s = |v: [f64; 3]| v.map(|c| c as f32);
        Light {
            direction: f32s(toward),
            strength: strength as f32,
            sun: f32s(direct),
            ambient: f32s(Self::ambient(self.elevation)),
            exposure: exposure as f32,
            glow_direction: f32s(direction(self.elevation.max(-6.0), self.azimuth)),
            glow: f32s(Self::color(self.elevation.max(MIN_ELEVATION)).map(|c| c * afterglow)),
            night: (night * strength) as f32,
            stars: (stars * strength) as f32,
        }
    }

    /// Brume et haut du ciel (RVBA de 0 à 1) teintés par le soleil, d'autant plus qu'il est bas ; sous
    /// l'horizon, ceux de l'heure bleue puis de la nuit.
    pub fn atmosphere(&self, haze: [f32; 4], sky: [f32; 4]) -> ([f32; 4], [f32; 4]) {
        let e = self.elevation;
        let strength = self.strength.clamp(0.0, 1.0);
        let tinted = |color: [f32; 4], sunset: [f64; 3], share: f64, blue: [f64; 3], night: [f64; 3]| {
            let base: [f64; 3] = std::array::from_fn(|i| f64::from(color[i]));
            let warmth = 1.0 - smoothstep(0.0, 25.0, e);
            let mut tint = mix3(base, sunset, warmth * share);
            if e < 0.0 {
                tint = mix3(tint, blue, smoothstep(0.0, BLUE_HOUR_ELEVATION, e));
                tint = mix3(tint, night, smoothstep(BLUE_HOUR_ELEVATION, NIGHT_ELEVATION, e));
            }
            let [r, g, b] = mix3(base, tint, strength).map(|v| v as f32);
            [r, g, b, color[3]]
        };
        (
            tinted(haze, SUNSET_HAZE, 0.85, BLUE_HOUR_HAZE, NIGHT_HAZE),
            tinted(sky, SUNSET_SKY, 0.7, BLUE_HOUR_SKY, NIGHT_SKY_COLOR),
        )
    }
}

/// Hauteur et direction (degrés, depuis le nord vers l'est) d'un astre d'angle horaire `hour` et de
/// déclinaison `delta`, vu à la latitude `phi` (radians).
fn position(hour: f64, delta: f64, phi: f64) -> (f64, f64) {
    let sin_elevation = phi.sin() * delta.sin() + phi.cos() * delta.cos() * hour.cos();
    let elevation = sin_elevation.clamp(-1.0, 1.0).asin();
    // Depuis le nord vers l'est : à l'est le matin (angle horaire négatif), au sud à midi.
    let azimuth = (-delta.cos() * hour.sin()).atan2((delta.sin() - sin_elevation * phi.sin()) / phi.cos());
    (elevation.to_degrees(), azimuth.to_degrees().rem_euclid(360.0))
}

/// Vers un astre, en repère local (x à l'est, y au nord, z en haut).
fn direction(elevation: f64, azimuth: f64) -> [f64; 3] {
    let (elevation, azimuth) = (elevation.to_radians(), azimuth.to_radians());
    [elevation.cos() * azimuth.sin(), elevation.cos() * azimuth.cos(), elevation.sin()]
}

fn mix(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn mix3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    std::array::from_fn(|i| mix(a[i], b[i], t))
}

/// Monte de 0 à 1 de `from` à `to`, dans un sens ou dans l'autre.
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

    fn at(course: f64) -> Sun {
        Sun::at(course, 45.0, 1.0)
    }

    #[test]
    fn follows_its_course_from_east_to_south_to_west() {
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
    fn sets_then_passes_under_the_north_at_midnight_and_rises_again() {
        // Couché à 180 et levé à 360 : sous l'horizon entre les deux, au plus bas au nord à minuit.
        assert!(at(180.0).elevation.abs() < 1.0 && at(360.0).elevation.abs() < 1.0);
        assert!((at(180.0).elevation - at(179.999).elevation).abs() < 1e-3, "course continue au coucher");
        let north = at(270.0).azimuth;
        assert!(north.min(360.0 - north) < 1e-6, "{:?}", at(270.0));
        assert!((at(270.0).elevation + 33.0).abs() < 1e-6, "{:?}", at(270.0));
        assert!(at(225.0).elevation < 0.0 && at(315.0).elevation < 0.0);
        // La pleine lune, en face, est au plus haut au sud à minuit.
        assert!((at(270.0).moon_azimuth - 180.0).abs() < 1e-6 && at(270.0).moon_elevation > 30.0);
    }

    #[test]
    fn the_low_sun_turns_red_and_the_light_stays_readable() {
        let high = Sun { elevation: 20.0, ..at(90.0) }.light();
        let low = Sun { elevation: 2.0, azimuth: 255.0, ..at(90.0) }.light();
        // Haut, doré ; rasant, rouge orangé.
        assert!(high.sun[2] / high.sun[0] > 0.35, "{:?}", high.sun);
        assert!(low.sun[2] / low.sun[0] < 0.05 && low.sun[1] / low.sun[0] < 0.4, "{:?}", low.sun);
        // L'exposition remonte la lumière du soleil couchant.
        assert!(low.exposure > high.exposure);
        // Vers l'ouest-sud-ouest, un peu au-dessus de l'horizon.
        assert!(low.direction[0] < -0.9 && low.direction[1] < 0.0 && low.direction[2] > 0.0);
    }

    /// Lumière d'un sol plat vu à l'écran : ce que l'exposition en garde.
    fn seen(sun: Sun) -> f64 {
        let light = sun.light();
        let flat = |c: [f32; 3]| luminance(c.map(f64::from));
        (flat(light.sun) * f64::from(light.direction[2]) + flat(light.ambient)) * f64::from(light.exposure)
    }

    #[test]
    fn the_night_falls_smoothly_dark_but_readable_under_the_moon() {
        // Pas à pas du coucher au lever : aucun saut de lumière.
        let courses: Vec<f64> = (0..=1800).map(|i| 170.0 + f64::from(i) * 0.1).collect();
        for pair in courses.windows(2) {
            let (a, b) = (seen(at(pair[0])), seen(at(pair[1])));
            assert!((a - b).abs() < 0.02 * a.max(b), "saut de lumière entre {} et {} : {a} → {b}", pair[0], pair[1]);
        }
        // À minuit, plus sombre que le jour mais lisible, éclairé par la lune.
        let (day, night) = (seen(at(90.0)), seen(at(270.0)));
        assert!(night < 0.4 * day && night > 0.25 * day, "jour {day}, nuit {night}");
        let midnight = at(270.0).light();
        assert!(midnight.direction[1] < -0.5 && midnight.direction[2] > 0.5, "{:?}", midnight.direction);
        assert!(midnight.sun[2] > midnight.sun[0], "lumière froide : {:?}", midnight.sun);
        assert!(midnight.night > 0.99 && midnight.stars > 0.99);
        // Au moment où la lumière passe du soleil à la lune, aucun des deux n'éclaire.
        let switch = Sun { elevation: SWITCH_ELEVATION, moon_elevation: 4.0, ..at(180.0) };
        assert!(luminance(Sun::sunlight(SWITCH_ELEVATION)) < 1e-3 && luminance(switch.moonlight()) < 1e-3);
    }

    #[test]
    fn the_sky_goes_from_sunset_to_blue_hour_to_night() {
        let base = ([0.87, 0.91, 0.94, 1.0], [0.5, 0.66, 0.84, 1.0]);
        let (sunset, _) = at(179.0).atmosphere(base.0, base.1);
        let (_, night) = at(270.0).atmosphere(base.0, base.1);
        assert!(sunset[0] > sunset[2], "brume orangée au couchant : {sunset:?}");
        assert!(night[2] > night[0] && night[2] < 0.1, "ciel bleu nuit : {night:?}");
    }
}
