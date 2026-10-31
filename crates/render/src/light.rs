//! Le soleil sur la vue inclinée (`camera.sunlight`) : à ses vraies positions aux heures du parcours
//! (`Sun::at_time`, avec la vraie lune et sa phase), ou sur une course stylisée du jour et de la nuit à la
//! latitude du parcours, de `camera.sunFrom` à `camera.sunTo` au fil de la vidéo (`Sun::at` ; 0 : lever,
//! 90 : midi, 180 : coucher, 270 : minuit, 360 : lever suivant, sous une pleine lune). Sa lumière traverse d'autant plus d'atmosphère qu'il
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
/// Lueur du ciel ajoutée la nuit à mesure que la lune manque (nouvelle, mince ou couchée) : la nuit la plus
/// noire garde les quatre cinquièmes de la lumière d'une nuit de pleine lune, le relief s'y lit encore.
const NIGHT_FLOOR: [f64; 3] = [0.0056, 0.0078, 0.0140];
/// Hauteur d'une pleine lune de référence, d'après laquelle la nuit s'expose : une lune plus basse, plus
/// mince ou absente assombrit vraiment la nuit.
const MOON_REFERENCE_ELEVATION: f64 = 30.0;
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
    /// La lune : hauteur, direction et éclat au regard de la pleine lune (0 à la nouvelle lune).
    pub moon_elevation: f64,
    pub moon_azimuth: f64,
    pub moon_brightness: f64,
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
        Sun { elevation, azimuth, moon_elevation, moon_azimuth, moon_brightness: 1.0, strength }
    }

    /// Le soleil et la lune vrais à l'instant `unix` (secondes UTC depuis 1970), vus depuis `[lon, lat]` :
    /// positions à un dixième de degré près pour le soleil, quelques dixièmes pour la lune (formules
    /// abrégées de l'Astronomical Almanac), éclat de la lune selon sa phase.
    pub fn at_time(unix: f64, [longitude, latitude]: [f64; 2], strength: f64) -> Sun {
        let phi = latitude.clamp(-89.9, 89.9).to_radians();
        // Jours depuis J2000.0 (1er janvier 2000, 12 h TT ; TT et UTC confondus) et siècles juliens.
        let days = unix / 86_400.0 + 2_440_587.5 - 2_451_545.0;
        let centuries = days / 36_525.0;
        let obliquity = (23.439 - 0.000_000_4 * days).to_radians();
        // Temps sidéral local : l'angle horaire d'un astre en découle.
        let sidereal = (280.460_618_37 + 360.985_647_366_29 * days + longitude).to_radians();
        let local = |right_ascension: f64, declination: f64| position(sidereal - right_ascension, declination, phi);

        // Soleil : longitude écliptique apparente.
        let mean_longitude = 280.460 + 0.985_647_4 * days;
        let anomaly = (357.528 + 0.985_600_3 * days).to_radians();
        let sun_longitude = (mean_longitude + 1.915 * anomaly.sin() + 0.020 * (2.0 * anomaly).sin()).to_radians();
        let (ra, dec) = equatorial(sun_longitude, 0.0, obliquity);
        let (elevation, azimuth) = local(ra, dec);

        // Lune : longitude et latitude écliptiques, puis parallaxe (vue de la surface, elle paraît plus bas).
        let term = |a: f64, b: f64| (a + b * centuries).to_radians().sin();
        let moon_longitude = 218.32 + 481_267.881 * centuries + 6.29 * term(135.0, 477_198.87)
            - 1.27 * term(259.3, -413_335.36)
            + 0.66 * term(235.7, 890_534.22)
            + 0.21 * term(269.9, 954_397.74)
            - 0.19 * term(357.5, 35_999.05)
            - 0.11 * term(186.5, 966_404.03);
        let moon_latitude = 5.13 * term(93.3, 483_202.02) + 0.28 * term(228.2, 960_400.89)
            - 0.28 * term(318.3, 6_003.15)
            - 0.17 * term(217.6, -407_332.21);
        let (moon_longitude, moon_latitude) = (moon_longitude.to_radians(), moon_latitude.to_radians());
        let (ra, dec) = equatorial(moon_longitude, moon_latitude, obliquity);
        let (moon_elevation, moon_azimuth) = local(ra, dec);
        let moon_elevation = moon_elevation - 0.95 * moon_elevation.to_radians().cos();

        // Phase : angle soleil–terre–lune vu de la lune, magnitude de la lune selon lui (Allen).
        let elongation = (moon_latitude.cos() * (moon_longitude - sun_longitude).cos()).clamp(-1.0, 1.0).acos();
        let phase_angle = 180.0 - elongation.to_degrees();
        let magnitude = 0.026 * phase_angle + 4e-9 * phase_angle.powi(4);
        let moon_brightness = 10f64.powf(-0.4 * magnitude);
        Sun { elevation, azimuth, moon_elevation, moon_azimuth, moon_brightness, strength }
    }

    /// Couleur de la lumière directe, blanche au zénith.
    fn color(elevation: f64) -> [f64; 3] {
        // Masse d'air de Kasten et Young, relative au zénith.
        let air_mass = 1.0 / (elevation.to_radians().sin() + 0.50572 * (elevation + 6.07995).powf(-1.6364));
        EXTINCTION.map(|tau| SUN_GAIN * (-tau * (air_mass - 1.0)).exp())
    }

    /// Lumière du ciel : du jour au crépuscule au-dessus de l'horizon, du crépuscule à la nuit dessous.
    fn sky(elevation: f64) -> [f64; 3] {
        if elevation >= 0.0 {
            mix3(TWILIGHT_SKY, DAY_SKY, smoothstep(0.0, 20.0, elevation))
        } else {
            mix3(TWILIGHT_SKY, NIGHT_SKY, smoothstep(0.0, MOONRISE[1], elevation))
        }
    }

    /// Lumière du ciel, et la nuit la lueur qui supplée une lune faible ou absente (`NIGHT_FLOOR`).
    fn ambient(&self) -> [f64; 3] {
        let night = smoothstep(MOONRISE[0], MOONRISE[1], self.elevation);
        let moon = self.moon_brightness * smoothstep(0.0, 6.0, self.moon_elevation);
        let sky = Self::sky(self.elevation);
        std::array::from_fn(|i| sky[i] + NIGHT_FLOOR[i] * night * (1.0 - moon.clamp(0.0, 1.0)))
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
        MOON.map(|c| c * night * risen * self.moon_brightness)
    }

    /// La lumière d'après laquelle s'expose l'image : celle d'un sol plat, la nuit sous la pleine lune de
    /// référence quelle que soit la vraie, qui l'éclaire plus ou moins.
    fn exposure_light(&self) -> f64 {
        let night = smoothstep(MOONRISE[0], MOONRISE[1], self.elevation);
        let sun = luminance(Self::sunlight(self.elevation)) * self.elevation.max(MIN_ELEVATION).to_radians().sin();
        let moon = luminance(MOON) * night * MOON_REFERENCE_ELEVATION.to_radians().sin();
        sun + moon + luminance(Self::sky(self.elevation))
    }

    pub fn light(&self) -> Light {
        // Un sol plat sous le soleil de référence garde ses couleurs ; plus bas, il n'en rattrape qu'une part.
        let reference = Sun { elevation: REFERENCE_ELEVATION, moon_elevation: -REFERENCE_ELEVATION, ..*self };
        let reference = reference.exposure_light();
        let night = smoothstep(MOONRISE[0], MOONRISE[1], self.elevation);
        let compensation = mix(EXPOSURE_COMPENSATION[0], EXPOSURE_COMPENSATION[1], night);
        let exposure = (reference / self.exposure_light()).powf(compensation) / reference;
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
            ambient: f32s(self.ambient()),
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

/// Ascension droite et déclinaison (radians) d'un astre de longitude et latitude écliptiques données.
fn equatorial(longitude: f64, latitude: f64, obliquity: f64) -> (f64, f64) {
    let right_ascension = (longitude.sin() * obliquity.cos() - latitude.tan() * obliquity.sin()).atan2(longitude.cos());
    let declination =
        (latitude.sin() * obliquity.cos() + latitude.cos() * obliquity.sin() * longitude.sin()).clamp(-1.0, 1.0).asin();
    (right_ascension, declination)
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

    /// Secondes UTC depuis 1970 d'une date et heure UTC.
    fn unix(year: i64, month: i64, day: i64, hour: i64, minute: i64) -> f64 {
        // Jours depuis 1970 (algorithme de Howard Hinnant).
        let y = if month <= 2 { year - 1 } else { year };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        (days * 86_400 + hour * 3_600 + minute * 60) as f64
    }

    const PARIS: [f64; 2] = [2.3522, 48.8566];

    #[test]
    fn the_real_sun_rises_and_sets_on_time_in_paris() {
        // Solstice 2026 : lever 5 h 47 et coucher 21 h 58 (heure d'été), le soleil 0,83° sous l'horizon
        // géométrique (réfraction et rayon du disque) ; au plus haut vers 13 h 52.
        let at = |hour, minute| Sun::at_time(unix(2026, 6, 21, hour, minute), PARIS, 1.0);
        assert!((at(3, 47).elevation + 0.83).abs() < 0.6, "{:?}", at(3, 47));
        assert!((at(19, 58).elevation + 0.83).abs() < 0.6, "{:?}", at(19, 58));
        assert!(at(3, 47).azimuth > 45.0 && at(3, 47).azimuth < 60.0, "lever au nord-est : {:?}", at(3, 47));
        assert!(at(19, 58).azimuth > 300.0 && at(19, 58).azimuth < 315.0, "{:?}", at(19, 58));
        assert!((at(11, 52).elevation - 64.6).abs() < 0.5 && (at(11, 52).azimuth - 180.0).abs() < 3.0);
        // Minuit : bien au-dessous, au nord.
        assert!(at(23, 52).elevation < -15.0 && (at(23, 52).azimuth < 10.0 || at(23, 52).azimuth > 350.0));
    }

    #[test]
    fn the_real_moon_has_its_phase() {
        let at = |day, hour, minute| Sun::at_time(unix(2025, 9, day, hour, minute), PARIS, 1.0);
        // Pleine lune de l'éclipse du 7 septembre 2025, nouvelle lune du 21, premier quartier du 29.
        let full = at(7, 18, 11);
        assert!(full.moon_brightness > 0.85, "{full:?}");
        assert!(at(21, 19, 54).moon_brightness < 0.01, "{:?}", at(21, 19, 54));
        let quarter = at(29, 23, 54).moon_brightness;
        assert!(quarter > 0.05 && quarter < 0.15, "{quarter}");
        // La pleine lune se lève quand le soleil se couche, et passe au sud dans la nuit.
        assert!(full.moon_elevation.abs() < 6.0 && full.elevation.abs() < 6.0, "{full:?}");
        assert!(full.moon_azimuth > 70.0 && full.moon_azimuth < 120.0, "{full:?}");
        let midnight = at(7, 23, 30);
        assert!(midnight.moon_elevation > 20.0 && (midnight.moon_azimuth - 180.0).abs() < 25.0, "{midnight:?}");
    }

    #[test]
    fn a_moonless_night_is_darker_but_still_readable() {
        let full = Sun { moon_elevation: 35.0, ..at(270.0) };
        let new = Sun { moon_brightness: 0.0, ..full };
        let set = Sun { moon_elevation: -10.0, ..full };
        let (full, new, set) = (seen(full), seen(new), seen(set));
        assert!(new < 0.9 * full && new > 0.7 * full, "pleine lune {full}, nouvelle lune {new}");
        assert!((set - new).abs() < 1e-9, "lune couchée comme nouvelle lune : {set} {new}");
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
