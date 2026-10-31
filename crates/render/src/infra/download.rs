//! Tout ce que le rendu lit sur le disque, téléchargé avant lui et gardé en cache sous `data/cache/` :
//! tuiles (imagerie, relief, tuiles vectorielles), polices de la carte d'infos, images des lieux. Le
//! moteur ne fait ensuite aucune requête, et les rendus suivants n'interrogent plus les fournisseurs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use dijkstravel_core::places::Place;

use crate::model::{ImagerySource, TileSource};
use crate::monitor::{CANCELLED, Monitor, Stage};

const USER_AGENT: &str = "dijkstravel/0.1 (outil personnel de cartographie)";
/// Essais par tuile, le premier compris ; attente avant le deuxième, doublée à chaque suivant.
const ATTEMPTS: u32 = 6;
const RETRY_DELAY: Duration = Duration::from_millis(500);
/// Requêtes simultanées vers les fournisseurs : EOX coupe des connexions au-delà de quelques dizaines.
const CONCURRENCY: usize = 8;
/// Imageries qui couvrent le monde entier, mer comprise : une tuile n'y manque jamais vraiment.
const WORLDWIDE_IMAGERIES: [&str; 2] = ["nasa", "sentinel"];
/// Pause avant de retenter les tuiles restées injoignables : le temps qu'un fournisseur nous relâche.
const SECOND_PASS_DELAY: Duration = Duration::from_secs(20);
/// Graisses de la carte d'infos : titre (700), texte (800), distance (900).
const HUD_WEIGHTS: [u16; 3] = [700, 800, 900];
/// Échec d'une requête : sa raison, et si un nouvel essai vaut la peine.
struct Failure {
    retry: bool,
    reason: String,
}

const IMAGE_EXTENSIONS: [&str; 5] = ["svg", "png", "jpg", "jpeg", "webp"];

struct Imagery {
    name: &'static str,
    tiles: &'static str,
    maxzoom: u8,
    credit: &'static str,
}

/// Fonds satellite : adresse chez le fournisseur (`{z}`, `{x}`, `{y}`), zoom max, crédit à afficher.
const IMAGERIES: [Imagery; 3] = [
    // NASA Blue Marble (domaine public), 500 m/pixel : volontairement peu détaillé, rien au-delà du zoom 8.
    Imagery {
        name: "nasa",
        tiles: "https://gibs.earthdata.nasa.gov/wmts/epsg3857/best/BlueMarble_NextGeneration/default/GoogleMapsCompatible_Level8/{z}/{y}/{x}.jpeg",
        maxzoom: 8,
        credit: "NASA Blue Marble",
    },
    // Orthophoto IGN (licence ouverte Etalab 2.0), 20 cm/pixel, France seulement.
    Imagery {
        name: "ign",
        tiles: "https://data.geopf.fr/wmts?SERVICE=WMTS&REQUEST=GetTile&VERSION=1.0.0&LAYER=ORTHOIMAGERY.ORTHOPHOTOS&STYLE=normal&TILEMATRIXSET=PM&FORMAT=image/jpeg&TILEMATRIX={z}&TILEROW={y}&TILECOL={x}",
        maxzoom: 19,
        credit: "Orthophoto © IGN",
    },
    // Sentinel-2 cloudless 2016 d'EOX, 10 m/pixel, mer comprise. Le millésime 2016 seulement : il est en
    // CC BY 4.0, les suivants en CC BY-NC-SA (pas d'usage commercial).
    Imagery {
        name: "sentinel",
        tiles: "https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless_3857/default/g/{z}/{y}/{x}.jpg",
        maxzoom: 14,
        credit: "Sentinel-2 cloudless by EOX (Copernicus Sentinel data 2016)",
    },
];

/// Relief mondial (Mapzen Terrain Tiles, encodage Terrarium), d'environ 30 m à la source en montagne :
/// partout où l'IGN n'a rien.
const TERRAIN: (&str, &str, u8) =
    ("terrarium", "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png", 14);
/// Relief de l'IGN (RGE ALTI, 1 à 5 m, licence ouverte Etalab 2.0), France et outre-mer : servi en WMS
/// à la taille et dans la projection de nos tuiles, en altitudes flottantes, -99999 hors couverture. Il
/// est rangé au cache en Terrarium, ses trous en vides que Terrarium comble.
const IGN_TERRAIN: (&str, u8) = ("ign-alti", 15);
const IGN_TERRAIN_WMS: &str = "https://data.geopf.fr/wms-r/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=ELEVATION.ELEVATIONGRIDCOVERAGE.HIGHRES&STYLES=&CRS=EPSG:3857&WIDTH=256&HEIGHT=256&FORMAT=image/x-bil;bits=32&BBOX=";
/// La métropole et la Corse, puis la Guadeloupe, la Martinique, la Guyane, La Réunion, Mayotte et
/// Saint-Pierre-et-Miquelon, larges : `[ouest, sud, est, nord]`.
const IGN_TERRAIN_BOUNDS: [[f64; 4]; 7] = [
    [-5.5, 41.2, 9.8, 51.2],
    [-61.9, 15.8, -60.9, 16.6],
    [-61.3, 14.3, -60.7, 14.9],
    [-54.7, 2.0, -51.5, 5.9],
    [55.1, -21.5, 55.9, -20.8],
    [44.9, -13.1, 45.4, -12.5],
    [-56.5, 46.7, -56.1, 47.2],
];
/// Tuiles vectorielles OpenFreeMap (schéma OpenMapTiles), dont seules les frontières sont tirées. Leur
/// adresse change à chaque mise à jour de la carte : elle se lit dans le TileJSON.
const VECTOR: (&str, &str, u8) = ("openmaptiles", "https://tiles.openfreemap.org/planet", 14);

/// Source d'imagerie d'un thème (`basemap.imagery`, déjà validé).
pub fn imagery_source(name: &str) -> ImagerySource {
    let imagery = IMAGERIES.iter().find(|imagery| imagery.name == name).unwrap_or(&IMAGERIES[0]);
    ImagerySource { name: imagery.name.to_owned(), maxzoom: imagery.maxzoom, credit: imagery.credit.to_owned() }
}

/// Le relief de l'IGN, comblé par Terrarium.
pub fn terrain_source() -> TileSource {
    let terrarium = TileSource { name: TERRAIN.0.to_owned(), maxzoom: TERRAIN.2, bounds: Vec::new(), fallback: None };
    TileSource {
        name: IGN_TERRAIN.0.to_owned(),
        maxzoom: IGN_TERRAIN.1,
        bounds: IGN_TERRAIN_BOUNDS.to_vec(),
        fallback: Some(Box::new(terrarium)),
    }
}

pub fn vector_source() -> TileSource {
    TileSource { name: VECTOR.0.to_owned(), maxzoom: VECTOR.2, bounds: Vec::new(), fallback: None }
}

/// Images des lieux dans le cache.
#[derive(Debug, Default)]
pub struct PlaceImages {
    /// Identifiant du lieu → fichier.
    pub files: HashMap<String, String>,
    /// Lieux dont l'image est introuvable : identifiant, adresse.
    pub missing: Vec<(String, String)>,
}

pub struct Downloader {
    cache_dir: PathBuf,
    agent: ureq::Agent,
    /// Lue une fois, et seulement s'il manque une tuile vectorielle : pas de réseau requis hors ligne.
    vector_template: OnceLock<Result<String, String>>,
}

impl Downloader {
    /// `cache_dir` : `data/cache`.
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .user_agent(USER_AGENT)
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .into();
        Downloader { cache_dir: cache_dir.into(), agent, vector_template: OnceLock::new() }
    }

    pub fn tile_dir(&self) -> PathBuf {
        self.cache_dir.join("tiles")
    }

    /// Corps de la réponse si elle est bonne ; sinon pourquoi, et si un nouvel essai vaut la peine
    /// (connexion coupée, refus passager, serveur en erreur) ou non (404, 410 : hors couverture). L'IGN
    /// refuse parfois une tuile qui existe d'une autre 4xx : la prendre pour un trou laissait un carré vide.
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Vec<u8>, Failure> {
        let mut request = self.agent.get(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let retry = |reason: String| Failure { retry: true, reason };
        let mut response = request.call().map_err(|e| retry(e.to_string()))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(Failure { retry: !matches!(status, 404 | 410), reason: format!("HTTP {status}") });
        }
        response.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|e| retry(e.to_string()))
    }

    /// `Ok(None)` : la ressource n'existe pas ; `Err` : toujours injoignable après `ATTEMPTS` essais.
    /// `worldwide` : une 4xx n'y veut pas dire « hors couverture » mais un refus passager (Sentinel-2 en
    /// rafale, l'Écosse restée noire), retenté comme une 5xx.
    fn get_with_retries(&self, url: &str, worldwide: bool) -> Result<Option<Vec<u8>>, String> {
        let mut reason = String::new();
        for attempt in 0..ATTEMPTS {
            if attempt > 0 {
                std::thread::sleep(RETRY_DELAY * 2u32.pow(attempt - 1));
            }
            match self.get(url, &[]) {
                Ok(body) => return Ok(Some(body)),
                Err(Failure { retry: false, .. }) if !worldwide => return Ok(None),
                Err(Failure { reason: last, .. }) => reason = last,
            }
        }
        Err(reason)
    }

    fn template(&self, source: &str) -> Result<String, String> {
        if let Some(imagery) = IMAGERIES.iter().find(|imagery| imagery.name == source) {
            return Ok(imagery.tiles.to_owned());
        }
        if source == TERRAIN.0 {
            return Ok(TERRAIN.1.to_owned());
        }
        if source != VECTOR.0 {
            return Err(format!("Source de tuiles inconnue : {source}"));
        }
        self.vector_template
            .get_or_init(|| {
                let body = self.get(VECTOR.1, &[]).map_err(|_| format!("{} : injoignable", VECTOR.1))?;
                let raw: serde_json::Value =
                    serde_json::from_slice(&body).map_err(|e| format!("{} : {e}", VECTOR.1))?;
                raw["tiles"][0]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("{} : pas d'adresse de tuiles", VECTOR.1))
            })
            .clone()
    }

    /// Tuiles `<source>/<z>/<x>/<y>` absentes du cache, téléchargées ; une tuile introuvable (hors
    /// couverture) reste absente et se dessine transparente. Une tuile injoignable est retentée après une
    /// pause, puis fait échouer le rendu : sinon elle se dessinerait en carré vide, sans un mot.
    pub fn fetch_tiles(&self, tiles: &[String], monitor: &dyn Monitor) -> Result<(), String> {
        let dir = self.tile_dir();
        let missing: Vec<&String> = tiles.iter().filter(|tile| !dir.join(tile).exists()).collect();
        let total = tiles.len() as u64;
        let done = AtomicUsize::new(tiles.len() - missing.len());
        let progress = || {
            let done = done.fetch_add(1, Ordering::Relaxed) as u64 + 1;
            if done < total {
                monitor.progress(Stage::Tiles, done, total);
            }
        };
        let mut failed = self.fetch_all(&missing, &dir, monitor, &progress)?;
        if !failed.is_empty() {
            std::thread::sleep(SECOND_PASS_DELAY);
            let again: Vec<&String> = failed.iter().map(|(tile, _)| *tile).collect();
            failed = self.fetch_all(&again, &dir, monitor, &|| {})?;
        }
        if let Some((tile, reason)) = failed.first() {
            return Err(format!("{} tuiles injoignables, dont {tile} ({reason}) : relancer le rendu", failed.len()));
        }
        monitor.progress(Stage::Tiles, total, total);
        Ok(())
    }

    /// Télécharge `tiles` en parallèle ; rend celles restées injoignables, avec la raison.
    fn fetch_all<'t>(
        &self,
        tiles: &[&'t String],
        dir: &Path,
        monitor: &dyn Monitor,
        progress: &(dyn Fn() + Sync),
    ) -> Result<Vec<(&'t String, String)>, String> {
        let next = AtomicUsize::new(0);
        let failed = Mutex::new(Vec::new());
        let errors: Vec<String> = std::thread::scope(|scope| {
            let workers: Vec<_> = (0..CONCURRENCY)
                .map(|_| {
                    scope.spawn(|| -> Result<(), String> {
                        while let Some(&tile) = tiles.get(next.fetch_add(1, Ordering::Relaxed)) {
                            if monitor.cancelled() {
                                return Err(CANCELLED.to_owned());
                            }
                            match self.fetch_tile(tile, dir)? {
                                Ok(()) => progress(),
                                Err(reason) => failed.lock().expect("tuiles en échec").push((tile, reason)),
                            }
                        }
                        Ok(())
                    })
                })
                .collect();
            workers.into_iter().filter_map(|worker| worker.join().expect("téléchargement").err()).collect()
        });
        match errors.into_iter().next() {
            Some(error) => Err(error),
            None => Ok(failed.into_inner().expect("tuiles en échec")),
        }
    }

    /// `Ok(Err(raison))` : la tuile est restée injoignable.
    fn fetch_tile(&self, tile: &str, dir: &Path) -> Result<Result<(), String>, String> {
        let parts: Vec<&str> = tile.split('/').collect();
        let [source, z, x, y] = parts.as_slice() else { return Err(format!("Tuile mal formée : {tile}")) };
        if *source == IGN_TERRAIN.0 {
            let parse = |value: &str| value.parse::<u32>().map_err(|_| format!("Tuile mal formée : {tile}"));
            let url = format!("{IGN_TERRAIN_WMS}{}", mercator_bbox(parse(z)?, parse(x)?, parse(y)?));
            return match self.get_with_retries(&url, false) {
                Ok(Some(body)) => write_atomically(&dir.join(tile), &terrarium_png(&body)?).map(Ok),
                Ok(None) => Ok(Ok(())),
                Err(reason) => Ok(Err(reason)),
            };
        }
        let url = self.template(source)?.replace("{z}", z).replace("{x}", x).replace("{y}", y);
        match self.get_with_retries(&url, WORLDWIDE_IMAGERIES.contains(source)) {
            Ok(Some(body)) => write_atomically(&dir.join(tile), &body).map(Ok),
            Ok(None) => Ok(Ok(())),
            Err(reason) => Ok(Err(reason)),
        }
    }

    /// Fichiers TTF de la police de la carte d'infos, une graisse par fichier ; une graisse que Google
    /// Fonts ne propose pas est remplacée par la plus proche au rendu.
    pub fn hud_fonts(&self, family: &str) -> Result<Vec<String>, String> {
        let dir = self.cache_dir.join("fonts");
        let files: Vec<String> = HUD_WEIGHTS
            .iter()
            .filter_map(|weight| {
                let file = dir.join(format!("{}-{weight}.ttf", family.replace(' ', "-")));
                (file.exists() || self.download_font(family, *weight, &file)).then(|| file.display().to_string())
            })
            .collect();
        if files.is_empty() {
            return Err(format!("Police {family} introuvable sur Google Fonts"));
        }
        Ok(files)
    }

    /// `false` si Google Fonts ne propose pas cette graisse (ou hors ligne).
    fn download_font(&self, family: &str, weight: u16, file: &Path) -> bool {
        let url = format!("https://fonts.googleapis.com/css2?family={}:wght@{weight}", family.replace(' ', "+"));
        // Ancien agent : Google Fonts répond par des TTF, que Skia charge, au lieu de WOFF2.
        let Ok(css) = self.get(&url, &[("user-agent", "Wget/1.21")]) else { return false };
        let css = String::from_utf8_lossy(&css);
        let Some(ttf_url) = css.split("src: url(").nth(1).and_then(|rest| rest.split(')').next()) else { return false };
        if !ttf_url.ends_with(".ttf") {
            return false;
        }
        self.get(ttf_url, &[]).ok().is_some_and(|body| write_atomically(file, &body).is_ok())
    }

    /// Images des lieux (`image` dans `places.geojson`), une fois dans `data/cache/images/`. Une image
    /// introuvable est rapportée dans `missing` et le lieu reste sans.
    pub fn place_images(&self, places: &[Place]) -> PlaceImages {
        let dir = self.cache_dir.join("images");
        let mut images = PlaceImages::default();
        // Séquentiel : upload.wikimedia.org répond 429 aux rafales.
        for place in places {
            let Some(url) = &place.image else { continue };
            match self.cached_image(url, &dir) {
                Some(file) => {
                    images.files.insert(place.id.clone(), file.display().to_string());
                }
                None => images.missing.push((place.id.clone(), url.clone())),
            }
        }
        images
    }

    fn cached_image(&self, url: &str, dir: &Path) -> Option<PathBuf> {
        let path = url.split(['?', '#']).next()?;
        let extension = path.rsplit_once('.')?.1.to_lowercase();
        if path.rsplit_once('/')?.1.rsplit_once('.').is_none() || !IMAGE_EXTENSIONS.contains(&extension.as_str()) {
            return None;
        }
        let file = dir.join(format!("{}.{extension}", sha1_smol::Sha1::from(url).digest()));
        if file.exists() {
            return Some(file);
        }
        for _ in 0..ATTEMPTS {
            let mut response = self.agent.get(url).call().ok()?;
            let status = response.status().as_u16();
            if status == 429 {
                let wait =
                    response.headers().get("retry-after").and_then(|v| v.to_str().ok()?.parse().ok()).unwrap_or(15);
                std::thread::sleep(Duration::from_secs(wait));
                continue;
            }
            if !(200..300).contains(&status) {
                return None;
            }
            let body = response.body_mut().with_config().limit(64 << 20).read_to_vec().ok()?;
            write_atomically(&file, &body).ok()?;
            return Some(file);
        }
        None
    }
}

/// Écrite à côté puis renommée : un rendu concurrent ne lit jamais un fichier à moitié écrit. Le nom
/// provisoire porte le processus et un compteur : deux rendus d'un même serveur ne s'y croisent pas.
/// `ouest,sud,est,nord` de la tuile `z/x/y`, en mètres Web Mercator (EPSG:3857).
fn mercator_bbox(z: u32, x: u32, y: u32) -> String {
    const HALF: f64 = 20_037_508.342_789_244;
    let side = 2.0 * HALF / f64::from(1u32 << z);
    let (west, north) = (-HALF + f64::from(x) * side, HALF - f64::from(y) * side);
    format!("{west},{},{},{north}", north - side, west + side)
}

/// Altitudes BIL (256 × 256 flottants 32 bits petit-boutistes) en PNG Terrarium : (R × 256 + G + B / 256)
/// − 32768 m, au 1/256 m près. Hors couverture (-99999), le vide de Terrarium (0, 0, 0).
fn terrarium_png(bil: &[u8]) -> Result<Vec<u8>, String> {
    const SIDE: usize = 256;
    if bil.len() != SIDE * SIDE * 4 {
        let start = String::from_utf8_lossy(&bil[..bil.len().min(200)]).into_owned();
        return Err(format!("relief IGN : {} octets au lieu de {} ({start})", bil.len(), SIDE * SIDE * 4));
    }
    let mut pixels = Vec::with_capacity(SIDE * SIDE * 4);
    for value in bil.as_chunks::<4>().0 {
        let altitude = f64::from(f32::from_le_bytes(*value));
        let encoded =
            if altitude.is_finite() && altitude > -1000.0 { (altitude + 32768.0).clamp(0.0, 65535.996) } else { 0.0 };
        let whole = encoded.floor();
        pixels.extend_from_slice(&[
            (whole / 256.0) as u8,
            (whole % 256.0) as u8,
            ((encoded - whole) * 256.0) as u8,
            255,
        ]);
    }
    let info = skia_safe::ImageInfo::new(
        (SIDE as i32, SIDE as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let image = skia_safe::images::raster_from_data(&info, skia_safe::Data::new_copy(&pixels), SIDE * 4)
        .ok_or("relief IGN : image impossible")?;
    let png = image.encode(None, skia_safe::EncodedImageFormat::PNG, 100).ok_or("relief IGN : PNG impossible")?;
    Ok(png.as_bytes().to_vec())
}

fn write_atomically(file: &Path, body: &[u8]) -> Result<(), String> {
    static PARTS: AtomicUsize = AtomicUsize::new(0);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{} : {e}", dir.display()))?;
    }
    let part = PARTS.fetch_add(1, Ordering::Relaxed);
    let partial = PathBuf::from(format!("{}.{}-{part}.part", file.display(), std::process::id()));
    std::fs::write(&partial, body).map_err(|e| format!("{} : {e}", partial.display()))?;
    std::fs::rename(&partial, file).map_err(|e| format!("{} : {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_the_ign_for_our_tile_in_web_mercator() {
        assert_eq!(
            mercator_bbox(0, 0, 0),
            "-20037508.342789244,-20037508.342789244,20037508.342789244,20037508.342789244"
        );
        assert_eq!(mercator_bbox(1, 1, 0), "0,0,20037508.342789244,20037508.342789244");
    }

    #[test]
    fn stores_ign_altitudes_as_terrarium_with_voids_outside_coverage() {
        let mut bil = Vec::new();
        for i in 0..256 * 256 {
            let altitude: f32 = if i == 0 { -99999.0 } else { 2454.5 };
            bil.extend_from_slice(&altitude.to_le_bytes());
        }
        let png = terrarium_png(&bil).unwrap();
        let image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(&png)).unwrap();
        let info =
            skia_safe::ImageInfo::new((256, 256), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
        let mut pixels = vec![0u8; 256 * 256 * 4];
        assert!(image.read_pixels(&info, &mut pixels, 256 * 4, (0, 0), skia_safe::image::CachingHint::Disallow));
        let altitude = |p: &[u8]| f64::from(p[0]) * 256.0 + f64::from(p[1]) + f64::from(p[2]) / 256.0 - 32768.0;
        assert_eq!(altitude(&pixels[0..4]), -32768.0, "vide de Terrarium");
        assert_eq!(altitude(&pixels[4..8]), 2454.5);
        assert!(terrarium_png(b"<ExceptionReport/>").is_err());
    }

    #[test]
    fn names_cached_images_like_before() {
        // Même nom que le cache rempli par l'ancienne chaîne Node : les blasons déjà téléchargés servent toujours.
        let url = "https://upload.wikimedia.org/wikipedia/commons/f/f6/Angers_Sporting_Club_de_l%27Ouest_logo.svg";
        assert_eq!(sha1_smol::Sha1::from(url).digest().to_string(), "919eb15cfc039b36a259224fc4430d668ca941f7");
    }

    #[test]
    fn falls_back_to_nasa_for_an_unknown_imagery() {
        assert_eq!(imagery_source("sentinel").maxzoom, 14);
        assert_eq!(imagery_source("bing").name, "nasa");
    }
}
