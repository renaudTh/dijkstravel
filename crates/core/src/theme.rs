//! Thème d'un run (`themes/<nom>.json`) : un fichier ne contient que ce qui change, le reste vient de
//! `Theme::default`, et un réglage ajouté plus tard prend sa valeur par défaut dans les anciens thèmes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Issue;
use crate::json::{field, finite};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    pub route: RouteTheme,
    pub stops: StopsTheme,
    pub basemap: BasemapTheme,
    pub hud: HudTheme,
    pub camera: CameraTheme,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteTheme {
    pub color: String,
    pub core_color: String,
    /// Largeur en pixels au zoom d'une région ; réduite au quart au zoom d'un pays.
    pub width: f64,
    /// Largeur du cœur rapportée à celle du trait, reprise pour les cercles des lieux atteints.
    pub core_ratio: f64,
    /// Flou du cœur rapporté à sa largeur.
    pub core_blur_ratio: f64,
    /// Trois bandes côte à côte (`color`, `core_color`, `stripe_color`) au lieu du cœur lumineux.
    pub stripes: bool,
    pub stripe_color: String,
    /// Couleur du trait sur les traversées en bateau ; celle du trait si le thème n'en donne pas.
    pub ferry_color: String,
    /// Rayon du point posé sur la tête du tracé, en pixels au zoom d'une région ; 0 : pas de point.
    pub head_radius: f64,
    /// Couleur de ce point, cerclé de `color` ; celle du cœur si le thème n'en donne pas.
    pub head_color: String,
    /// Emoji à la place du point (⚽…), de diamètre deux fois `head_radius` ; vide : le point.
    pub head_icon: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopsTheme {
    pub unvisited_color: String,
    pub unvisited_radius: f64,
    /// Rayon d'un lieu atteint, rapporté à la largeur du trait.
    pub visited_ratio: f64,
    /// Rayon ajouté au moment où le lieu est atteint (« pop »).
    pub pop_radius: f64,
    /// Emoji posé sur chaque lieu atteint (🏟️, 🍺…), qui « pope » avec lui ; vide : aucun.
    pub icon: String,
    pub icon_size: f64,
    /// Image du lieu (blason…) posée sur la carte à la place de l'emoji, quand ses lieux en ont une.
    pub show_image_on_map: bool,
    /// Taille de cette image en pixels, fixe : elle doit rester lisible à toutes les échelles.
    pub image_size: f64,
    /// Nom du lieu écrit sur la carte dès qu'il est atteint, sauf s'il en chevaucherait un autre.
    pub show_labels: bool,
    pub label_size: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BasemapTheme {
    /// Une de `IMAGERIES`.
    pub imagery: String,
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    pub hillshade: f64,
    pub borders: bool,
    pub border_color: String,
    /// Voile de couleur posé sur l'imagerie : une ambiance (nuit, sépia…) sans toucher au tracé.
    pub tint_color: String,
    pub tint_opacity: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HudTheme {
    /// Une de `HUD_FONTS`.
    pub font: String,
    /// Taille de base du texte de la carte d'infos, en pixels.
    pub size: f64,
    pub background: String,
    pub text_color: String,
    pub title_color: String,
    pub distance_color: String,
    pub pill_from: String,
    pub pill_to: String,
    pub pill_text_color: String,
    pub show_title: bool,
    pub show_count: bool,
    pub show_last_stop: bool,
    /// Image du dernier lieu (blason, logo…) quand ses lieux en ont une.
    pub show_image: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraTheme {
    /// Part de l'emprise totale visible pendant le parcours.
    pub follow_fraction: f64,
    /// Ralentissement au passage d'un lieu, rattrapé entre les lieux (2 : trois fois plus lent) ; 0 : allure constante.
    pub stop_slowdown: f64,
}

/// Imagerie du fond : `nasa` (Blue Marble, pays et régions), `ign` (orthophoto, villes de France),
/// `sentinel` (Sentinel-2 cloudless, monde entier mer comprise : îles et littoraux).
pub const IMAGERIES: [&str; 3] = ["nasa", "ign", "sentinel"];

/// Polices arrondies de Google Fonts proposées dans l'éditeur.
pub const HUD_FONTS: [&str; 6] = ["Nunito", "Quicksand", "Fredoka", "Baloo 2", "Poppins", "Comfortaa"];

/// Bornes des réglages numériques, partagées par la validation et les curseurs de l'éditeur.
pub const NUMBER_RANGES: [(&str, f64, f64); 18] = [
    ("route.width", 2.0, 40.0),
    ("route.coreRatio", 0.0, 1.0),
    ("route.coreBlurRatio", 0.0, 1.0),
    ("route.headRadius", 0.0, 40.0),
    ("stops.unvisitedRadius", 0.0, 20.0),
    ("stops.visitedRatio", 0.3, 2.0),
    ("stops.popRadius", 0.0, 40.0),
    ("stops.iconSize", 16.0, 160.0),
    ("stops.imageSize", 24.0, 200.0),
    ("stops.labelSize", 12.0, 60.0),
    ("basemap.brightness", 0.0, 1.0),
    ("basemap.contrast", -1.0, 1.0),
    ("basemap.saturation", -1.0, 1.0),
    ("basemap.hillshade", 0.0, 1.0),
    ("basemap.tintOpacity", 0.0, 0.9),
    ("hud.size", 12.0, 60.0),
    ("camera.followFraction", 0.02, 1.0),
    ("camera.stopSlowdown", 0.0, 5.0),
];

impl Default for Theme {
    fn default() -> Self {
        let s = String::from;
        Theme {
            route: RouteTheme {
                color: s("#702963"),
                core_color: s("#e0679f"),
                width: 16.0,
                core_ratio: 7.0 / 16.0,
                core_blur_ratio: 4.0 / 7.0,
                stripes: false,
                stripe_color: s("#ffffff"),
                ferry_color: s("#702963"),
                head_radius: 0.0,
                head_color: s("#e0679f"),
                head_icon: s(""),
            },
            stops: StopsTheme {
                unvisited_color: s("#d9d9d9"),
                unvisited_radius: 4.0,
                visited_ratio: 0.75,
                pop_radius: 12.0,
                icon: s(""),
                icon_size: 48.0,
                show_image_on_map: false,
                image_size: 70.0,
                show_labels: false,
                label_size: 28.0,
            },
            basemap: BasemapTheme {
                imagery: s("nasa"),
                brightness: 0.06,
                contrast: 0.15,
                saturation: 0.15,
                hillshade: 0.4,
                borders: true,
                border_color: s("#ffffff"),
                tint_color: s("#0b1630"),
                tint_opacity: 0.0,
            },
            hud: HudTheme {
                font: s("Nunito"),
                size: 30.0,
                background: s("#260c28"),
                text_color: s("#ffffff"),
                title_color: s("#f5a3c7"),
                distance_color: s("#e0679f"),
                pill_from: s("#702963"),
                pill_to: s("#e0679f"),
                pill_text_color: s("#ffffff"),
                show_title: true,
                show_count: true,
                show_last_stop: true,
                show_image: true,
            },
            camera: CameraTheme { follow_fraction: 1.0 / 8.0, stop_slowdown: 0.0 },
        }
    }
}

/// Lit les réglages présents dans le JSON brut ; chacun garde sa valeur par défaut s'il manque.
struct Reader<'a> {
    raw: &'a Value,
    issues: Vec<Issue>,
}

impl Reader<'_> {
    fn given(&self, path: &str) -> Option<&Value> {
        let (section, key) = path.split_once('.')?;
        field(field(self.raw, section)?, key)
    }

    fn read<T>(&mut self, path: &str, fallback: T, parse: impl Fn(&Value) -> Option<T>, expected: &str) -> T {
        let Some(value) = self.given(path) else { return fallback };
        match parse(value) {
            Some(parsed) => parsed,
            None => {
                self.issues.push(Issue::new(path, expected));
                fallback
            }
        }
    }

    fn color(&mut self, path: &str, fallback: &str) -> String {
        let valid = |value: &Value| {
            value
                .as_str()
                .filter(|text| {
                    text.len() == 7 && text.starts_with('#') && text[1..].chars().all(|c| c.is_ascii_hexdigit())
                })
                .map(String::from)
        };
        self.read(path, fallback.to_owned(), valid, "couleur #rrggbb")
    }

    /// Un emoji ou deux, pas un texte : il est dessiné en image sur la carte.
    fn emoji(&mut self, path: &str, fallback: &str) -> String {
        let valid = |value: &Value| value.as_str().filter(|text| text.encode_utf16().count() <= 16).map(String::from);
        self.read(path, fallback.to_owned(), valid, "emoji (16 unités UTF-16 au plus) ou vide")
    }

    fn flag(&mut self, path: &str, fallback: bool) -> bool {
        self.read(path, fallback, Value::as_bool, "booléen")
    }

    fn number(&mut self, path: &str, fallback: f64) -> f64 {
        let (min, max) = range(path);
        let valid = |value: &Value| finite(value).filter(|number| (min..=max).contains(number));
        self.read(path, fallback, valid, &format!("nombre entre {min} et {max}"))
    }

    fn choice(&mut self, path: &str, fallback: &str, choices: &[&str]) -> String {
        let valid = |value: &Value| value.as_str().filter(|text| choices.contains(text)).map(String::from);
        self.read(path, fallback.to_owned(), valid, &choices.join(" | "))
    }
}

fn range(path: &str) -> (f64, f64) {
    NUMBER_RANGES.iter().find(|(known, ..)| *known == path).map(|&(_, min, max)| (min, max)).expect("réglage borné")
}

impl Theme {
    /// Complète un thème partiel avec `Theme::default`.
    pub fn parse(raw: &Value) -> Result<Theme, Vec<Issue>> {
        if !raw.is_object() {
            return Err(vec![Issue::new("", "objet")]);
        }
        let d = Theme::default();
        let mut r = Reader { raw, issues: Vec::new() };
        let route_color = r.color("route.color", &d.route.color);
        let core_color = r.color("route.coreColor", &d.route.core_color);
        let theme = Theme {
            route: RouteTheme {
                width: r.number("route.width", d.route.width),
                core_ratio: r.number("route.coreRatio", d.route.core_ratio),
                core_blur_ratio: r.number("route.coreBlurRatio", d.route.core_blur_ratio),
                stripes: r.flag("route.stripes", d.route.stripes),
                stripe_color: r.color("route.stripeColor", &d.route.stripe_color),
                ferry_color: r.color("route.ferryColor", &route_color),
                head_radius: r.number("route.headRadius", d.route.head_radius),
                head_color: r.color("route.headColor", &core_color),
                head_icon: r.emoji("route.headIcon", &d.route.head_icon),
                color: route_color,
                core_color,
            },
            stops: StopsTheme {
                unvisited_color: r.color("stops.unvisitedColor", &d.stops.unvisited_color),
                unvisited_radius: r.number("stops.unvisitedRadius", d.stops.unvisited_radius),
                visited_ratio: r.number("stops.visitedRatio", d.stops.visited_ratio),
                pop_radius: r.number("stops.popRadius", d.stops.pop_radius),
                icon: r.emoji("stops.icon", &d.stops.icon),
                icon_size: r.number("stops.iconSize", d.stops.icon_size),
                show_image_on_map: r.flag("stops.showImageOnMap", d.stops.show_image_on_map),
                image_size: r.number("stops.imageSize", d.stops.image_size),
                show_labels: r.flag("stops.showLabels", d.stops.show_labels),
                label_size: r.number("stops.labelSize", d.stops.label_size),
            },
            basemap: BasemapTheme {
                imagery: r.choice("basemap.imagery", &d.basemap.imagery, &IMAGERIES),
                brightness: r.number("basemap.brightness", d.basemap.brightness),
                contrast: r.number("basemap.contrast", d.basemap.contrast),
                saturation: r.number("basemap.saturation", d.basemap.saturation),
                hillshade: r.number("basemap.hillshade", d.basemap.hillshade),
                borders: r.flag("basemap.borders", d.basemap.borders),
                border_color: r.color("basemap.borderColor", &d.basemap.border_color),
                tint_color: r.color("basemap.tintColor", &d.basemap.tint_color),
                tint_opacity: r.number("basemap.tintOpacity", d.basemap.tint_opacity),
            },
            hud: HudTheme {
                font: r.choice("hud.font", &d.hud.font, &HUD_FONTS),
                size: r.number("hud.size", d.hud.size),
                background: r.color("hud.background", &d.hud.background),
                text_color: r.color("hud.textColor", &d.hud.text_color),
                title_color: r.color("hud.titleColor", &d.hud.title_color),
                distance_color: r.color("hud.distanceColor", &d.hud.distance_color),
                pill_from: r.color("hud.pillFrom", &d.hud.pill_from),
                pill_to: r.color("hud.pillTo", &d.hud.pill_to),
                pill_text_color: r.color("hud.pillTextColor", &d.hud.pill_text_color),
                show_title: r.flag("hud.showTitle", d.hud.show_title),
                show_count: r.flag("hud.showCount", d.hud.show_count),
                show_last_stop: r.flag("hud.showLastStop", d.hud.show_last_stop),
                show_image: r.flag("hud.showImage", d.hud.show_image),
            },
            camera: CameraTheme {
                follow_fraction: r.number("camera.followFraction", d.camera.follow_fraction),
                stop_slowdown: r.number("camera.stopSlowdown", d.camera.stop_slowdown),
            },
        };
        if r.issues.is_empty() { Ok(theme) } else { Err(r.issues) }
    }
}

/// Lettres, chiffres, `-` et `_` : un nom de fichier sous `themes/`.
pub fn is_theme_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn theme_path(name: &str) -> String {
    format!("themes/{name}.json")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn completes_an_empty_theme_with_defaults() {
        assert_eq!(Theme::parse(&json!({})), Ok(Theme::default()));
    }

    #[test]
    fn keeps_given_values_and_defaults_the_rest() {
        let theme = Theme::parse(&json!({ "route": { "color": "#112233", "width": 10 } })).unwrap();
        assert_eq!(theme.route.color, "#112233");
        assert_eq!(theme.route.width, 10.0);
        assert_eq!(theme.route.core_color, Theme::default().route.core_color);
    }

    #[test]
    fn derives_ferry_and_head_colors_from_the_route() {
        let theme = Theme::parse(&json!({ "route": { "color": "#112233", "coreColor": "#445566" } })).unwrap();
        assert_eq!(theme.route.ferry_color, "#112233");
        assert_eq!(theme.route.head_color, "#445566");
    }

    #[test]
    fn reports_every_invalid_value_with_its_path() {
        let issues = Theme::parse(&json!({
            "route": { "color": "red", "width": 100 },
            "basemap": { "imagery": "bing" },
            "hud": { "showTitle": "oui" }
        }))
        .unwrap_err();
        let paths: Vec<&str> = issues.iter().map(|issue| issue.path.as_str()).collect();
        assert_eq!(paths, ["route.color", "route.width", "basemap.imagery", "hud.showTitle"]);
        assert_eq!(issues[1].expected, "nombre entre 2 et 40");
    }

    #[test]
    fn rejects_a_root_that_is_not_an_object() {
        assert_eq!(Theme::parse(&json!([])), Err(vec![Issue::new("", "objet")]));
    }

    #[test]
    fn limits_emojis_to_sixteen_utf16_units() {
        assert!(Theme::parse(&json!({ "stops": { "icon": "🏟️" } })).is_ok());
        assert!(Theme::parse(&json!({ "stops": { "icon": "un texte bien trop long" } })).is_err());
    }

    #[test]
    fn every_theme_in_the_repository_is_valid() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../themes");
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let raw: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert!(Theme::parse(&raw).is_ok(), "{}", path.display());
        }
    }
}
