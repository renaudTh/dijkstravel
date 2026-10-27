//! Un run : `runs/<nom>/run.json`, à côté de ses lieux (`places.geojson`) et de sa tournée (`tour.json`),
//! tous versionnés. Le run porte toute son information, provenance des lieux comprise : les scripts qui
//! les ont produits sont jetables.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::Issue;
use crate::geo::{BBox, LonLat};
use crate::json::{field, finite, lon_lat, non_empty};
use crate::theme::is_theme_name;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoFormat {
    Landscape,
    Portrait,
}

impl VideoFormat {
    pub fn size(self) -> (u32, u32) {
        match self {
            VideoFormat::Landscape => (1920, 1080),
            VideoFormat::Portrait => (1080, 1920),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoSettings {
    pub format: VideoFormat,
    pub duration_sec: f64,
    pub intro_sec: f64,
    pub outro_sec: f64,
    pub fps: u32,
}

impl Default for VideoSettings {
    fn default() -> Self {
        VideoSettings { format: VideoFormat::Landscape, duration_sec: 60.0, intro_sec: 3.0, outro_sec: 3.0, fps: 30 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Start {
    Free,
    Place(String),
    Coordinates(LonLat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TourMode {
    Loop,
    Open,
}

/// Profil de routage : chacun a son OSRM, avec ses propres données préparées (voir `docker-compose.yml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Car,
    Foot,
    Train,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverpassSource {
    /// Code ISO 3166 de la zone administrative, ex. `FR` ou `FR-BRE`.
    pub area_iso: String,
    /// Sélecteur de tags Overpass QL, ex. `["historic"="castle"]`.
    pub filter: String,
}

/// Légende et hashtags pour les réseaux ; la légende accepte `{lieux}` et `{km}`.
#[derive(Debug, Clone, PartialEq)]
pub struct Publication {
    pub caption: String,
    pub hashtags: Vec<String>,
}

pub const SOURCE_KINDS: [&str; 4] = ["overpass", "csv", "atp", "manuel"];

/// D'où viennent les lieux : ce qu'il faut pour les refaire, une fois le script jeté.
#[derive(Debug, Clone, PartialEq)]
pub struct Provenance {
    /// Une de `SOURCE_KINDS`.
    pub kind: String,
    pub description: String,
    pub url: Option<String>,
    /// Date du relevé, `AAAA-MM-JJ`.
    pub date: String,
    pub licence: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub name: String,
    /// Titre affiché dans la carte d'infos.
    pub title: Option<String>,
    pub source: Provenance,
    pub bbox: BBox,
    pub start: Start,
    pub mode: TourMode,
    pub profile: Profile,
    /// Nom du thème (`themes/<nom>.json`), `default` s'il n'en déclare pas.
    pub theme: String,
    pub video: VideoSettings,
    pub overpass: Option<OverpassSource>,
    pub publication: Option<Publication>,
}

/// `dir` : le run versionné ; `out` : ce qui s'en déduit (vidéo, couverture…), hors dépôt.
#[derive(Debug, Clone, PartialEq)]
pub struct RunPaths {
    pub file: PathBuf,
    pub dir: PathBuf,
    pub places: PathBuf,
    pub tour: PathBuf,
    pub out: PathBuf,
    pub video: PathBuf,
}

impl RunPaths {
    /// `runs/<nom>` ou `runs/<nom>/run.json` ; les sorties vont dans `<racine>/data/out/<nom>`, la racine
    /// étant le dossier courant.
    pub fn of(arg: impl AsRef<Path>, name: &str) -> RunPaths {
        let arg = arg.as_ref();
        let file = if arg.extension().is_some_and(|ext| ext == "json") { arg.to_owned() } else { arg.join("run.json") };
        let dir = file.parent().map_or_else(PathBuf::new, Path::to_owned);
        let out = Path::new("data/out").join(name);
        RunPaths {
            places: dir.join("places.geojson"),
            tour: dir.join("tour.json"),
            video: out.join("video.mp4"),
            file,
            dir,
            out,
        }
    }
}

pub fn run_file(arg: impl AsRef<Path>) -> PathBuf {
    RunPaths::of(arg, "").file
}

impl Run {
    pub fn parse(raw: &Value) -> Result<Run, Vec<Issue>> {
        if !raw.is_object() {
            return Err(vec![Issue::new("", "objet")]);
        }
        let mut issues = Vec::new();
        let mut check = |valid: bool, path: &str, expected: &str| {
            if !valid {
                issues.push(Issue::new(path, expected));
            }
        };
        let get = |key: &str| field(raw, key);

        let name = get("name")
            .and_then(non_empty)
            .filter(|name| name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')));
        check(name.is_some(), "name", "nom de dossier");
        let source = parse_provenance(get("source"));
        check(source.is_some(), "source", "{ kind, description, url?, date, licence }");
        let bbox = get("bbox").and_then(parse_bbox);
        check(bbox.is_some(), "bbox", "{ west, south, east, north }");
        let start = parse_start(get("start"));
        check(start.is_some(), "start", "{ place } | { lon, lat } | absent");
        let mode = match get("mode").and_then(Value::as_str) {
            Some("loop") => Some(TourMode::Loop),
            Some("open") => Some(TourMode::Open),
            _ => None,
        };
        check(mode.is_some(), "mode", "'loop' | 'open'");
        let profile = match get("profile").map(Value::as_str) {
            None | Some(Some("car")) => Some(Profile::Car),
            Some(Some("foot")) => Some(Profile::Foot),
            Some(Some("train")) => Some(Profile::Train),
            Some(_) => None,
        };
        check(profile.is_some(), "profile", "car | foot | train");
        let theme = match get("theme") {
            None => Some("default"),
            Some(value) => value.as_str().filter(|name| is_theme_name(name)),
        };
        check(theme.is_some(), "theme", "nom de fichier (lettres, chiffres, - et _)");
        let overpass = match get("overpass") {
            None => Some(None),
            Some(value) => {
                match (field(value, "areaIso").and_then(non_empty), field(value, "filter").and_then(non_empty)) {
                    (Some(area_iso), Some(filter)) => {
                        Some(Some(OverpassSource { area_iso: area_iso.to_owned(), filter: filter.to_owned() }))
                    }
                    _ => None,
                }
            }
        };
        check(overpass.is_some(), "overpass", "{ areaIso, filter } | absent");
        let video = parse_video(get("video"), &mut issues);

        match (name, source, bbox, start, mode, profile, theme, overpass, video) {
            (
                Some(name),
                Some(source),
                Some(bbox),
                Some(start),
                Some(mode),
                Some(profile),
                Some(theme),
                Some(overpass),
                Some(video),
            ) if issues.is_empty() => Ok(Run {
                name: name.to_owned(),
                title: get("title").and_then(non_empty).map(|title| title.trim().to_owned()),
                source,
                bbox,
                start,
                mode,
                profile,
                theme: theme.to_owned(),
                video,
                overpass,
                publication: parse_publication(get("publication")),
            }),
            _ => Err(issues),
        }
    }
}

fn parse_provenance(raw: Option<&Value>) -> Option<Provenance> {
    let raw = raw?;
    let text = |key: &str| field(raw, key).and_then(non_empty).map(str::to_owned);
    let date = text("date").filter(|date| {
        date.len() == 10
            && date.char_indices().all(|(i, c)| if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() })
    })?;
    Some(Provenance {
        kind: text("kind").filter(|kind| SOURCE_KINDS.contains(&kind.as_str()))?,
        description: text("description")?,
        url: match field(raw, "url") {
            None => None,
            Some(url) => Some(non_empty(url)?.to_owned()),
        },
        date,
        licence: text("licence")?,
    })
}

fn parse_bbox(raw: &Value) -> Option<BBox> {
    let side = |key: &str| field(raw, key).and_then(finite);
    let bbox = BBox { west: side("west")?, south: side("south")?, east: side("east")?, north: side("north")? };
    (bbox.west < bbox.east && bbox.south < bbox.north).then_some(bbox)
}

fn parse_start(raw: Option<&Value>) -> Option<Start> {
    let Some(raw) = raw else { return Some(Start::Free) };
    if let Some(place) = field(raw, "place").and_then(non_empty) {
        return Some(Start::Place(place.to_owned()));
    }
    let at = Value::Array(vec![field(raw, "lon")?.clone(), field(raw, "lat")?.clone()]);
    lon_lat(&at).map(Start::Coordinates)
}

fn parse_video(raw: Option<&Value>, issues: &mut Vec<Issue>) -> Option<VideoSettings> {
    let empty = Value::Object(serde_json::Map::new());
    let raw = raw.unwrap_or(&empty);
    if !raw.is_object() {
        issues.push(Issue::new("video", "objet"));
        return None;
    }
    let d = VideoSettings::default();
    let before = issues.len();
    let mut number = |key: &str, fallback: f64, valid: fn(f64) -> bool, expected: &str| -> f64 {
        match field(raw, key) {
            None => fallback,
            Some(value) => finite(value).filter(|&n| valid(n)).unwrap_or_else(|| {
                issues.push(Issue::new(format!("video.{key}"), expected));
                fallback
            }),
        }
    };
    let duration_sec = number("durationSec", d.duration_sec, |n| n > 0.0, "nombre > 0");
    let intro_sec = number("introSec", d.intro_sec, |n| n >= 0.0, "nombre >= 0");
    let outro_sec = number("outroSec", d.outro_sec, |n| n >= 0.0, "nombre >= 0");
    let fps = number("fps", f64::from(d.fps), |n| n > 0.0 && n.fract() == 0.0, "entier > 0");
    let format = match field(raw, "format").map(Value::as_str) {
        None | Some(Some("16:9")) => VideoFormat::Landscape,
        Some(Some("9:16")) => VideoFormat::Portrait,
        Some(_) => {
            issues.push(Issue::new("video.format", "'16:9' | '9:16'"));
            VideoFormat::Landscape
        }
    };
    (issues.len() == before).then_some(VideoSettings { format, duration_sec, intro_sec, outro_sec, fps: fps as u32 })
}

/// Sans légende, pas de publication : les hashtags seuls ne servent à rien.
fn parse_publication(raw: Option<&Value>) -> Option<Publication> {
    let raw = raw?;
    let caption = field(raw, "caption").and_then(non_empty)?.to_owned();
    let hashtags = field(raw, "hashtags")
        .and_then(Value::as_array)
        .map(|tags| tags.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    Some(Publication { caption, hashtags })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn minimal() -> Value {
        json!({
            "name": "bretagne",
            "source": { "kind": "manuel", "description": "relevé à la main", "date": "2026-10-27", "licence": "CC0" },
            "bbox": { "west": -5.2, "south": 47.2, "east": -1.0, "north": 48.95 },
            "mode": "loop"
        })
    }

    fn with(key: &str, value: Value) -> Value {
        let mut raw = minimal();
        raw[key] = value;
        raw
    }

    fn issue_paths(raw: &Value) -> Vec<String> {
        Run::parse(raw).unwrap_err().into_iter().map(|issue| issue.path).collect()
    }

    #[test]
    fn fills_defaults_for_a_minimal_run() {
        let run = Run::parse(&minimal()).unwrap();
        assert_eq!(run.start, Start::Free);
        assert_eq!(run.mode, TourMode::Loop);
        assert_eq!(run.profile, Profile::Car);
        assert_eq!(run.theme, "default");
        assert_eq!(run.video, VideoSettings::default());
        assert_eq!(run.overpass, None);
        assert_eq!(run.title, None);
        assert_eq!(run.publication, None);
        assert_eq!(run.source.url, None);
    }

    #[test]
    fn reads_every_optional_section() {
        let mut raw = minimal();
        raw["start"] = json!({ "place": "rennes" });
        raw["profile"] = json!("foot");
        raw["theme"] = json!("foot");
        raw["title"] = json!("  Les châteaux  ");
        raw["video"] = json!({ "format": "9:16", "durationSec": 30, "fps": 25 });
        raw["overpass"] = json!({ "areaIso": "FR-BRE", "filter": "[\"historic\"=\"castle\"]" });
        raw["publication"] = json!({ "caption": "{lieux} châteaux", "hashtags": ["#bzh", 3] });
        let run = Run::parse(&raw).unwrap();
        assert_eq!(run.start, Start::Place("rennes".into()));
        assert_eq!(run.profile, Profile::Foot);
        assert_eq!(run.title.as_deref(), Some("Les châteaux"));
        assert_eq!(run.video.format.size(), (1080, 1920));
        assert_eq!((run.video.duration_sec, run.video.fps, run.video.intro_sec), (30.0, 25, 3.0));
        assert_eq!(run.overpass.unwrap().area_iso, "FR-BRE");
        assert_eq!(run.publication.unwrap().hashtags, ["#bzh"]);
    }

    #[test]
    fn reads_a_start_by_coordinates() {
        let run = Run::parse(&with("start", json!({ "lon": -1.68, "lat": 48.11 }))).unwrap();
        assert_eq!(run.start, Start::Coordinates([-1.68, 48.11]));
    }

    #[test]
    fn reports_each_invalid_field() {
        assert_eq!(issue_paths(&with("name", json!("../etc"))), ["name"]);
        assert_eq!(issue_paths(&with("bbox", json!({ "west": 1, "south": 0, "east": 0, "north": 1 }))), ["bbox"]);
        assert_eq!(issue_paths(&with("mode", json!("aller"))), ["mode"]);
        assert_eq!(issue_paths(&with("profile", json!("bike"))), ["profile"]);
        assert_eq!(issue_paths(&with("theme", json!("../x"))), ["theme"]);
        assert_eq!(issue_paths(&with("overpass", json!({ "areaIso": "FR" }))), ["overpass"]);
        assert_eq!(issue_paths(&with("video", json!({ "fps": 2.5, "format": "4:3" }))), ["video.fps", "video.format"]);
    }

    #[test]
    fn requires_a_provenance() {
        let mut raw = minimal();
        raw.as_object_mut().unwrap().remove("source");
        assert_eq!(issue_paths(&raw), ["source"]);
        let bad_kind = json!({ "kind": "wiki", "description": "x", "date": "2026-10-27", "licence": "CC0" });
        assert_eq!(issue_paths(&with("source", bad_kind)), ["source"]);
        let bad_date = json!({ "kind": "csv", "description": "x", "date": "27/10/2026", "licence": "CC0" });
        assert_eq!(issue_paths(&with("source", bad_date)), ["source"]);
    }

    #[test]
    fn locates_files_from_the_run_folder() {
        let from_dir = RunPaths::of("runs/bretagne", "bretagne");
        assert_eq!(from_dir, RunPaths::of("runs/bretagne/run.json", "bretagne"));
        assert_eq!(from_dir.places, Path::new("runs/bretagne/places.geojson"));
        assert_eq!(from_dir.tour, Path::new("runs/bretagne/tour.json"));
        assert_eq!(from_dir.video, Path::new("data/out/bretagne/video.mp4"));
    }

    #[test]
    fn every_run_in_the_repository_is_valid() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../runs");
        for entry in std::fs::read_dir(dir).unwrap() {
            let file = entry.unwrap().path().join("run.json");
            let raw: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
            let run = Run::parse(&raw).unwrap_or_else(|issues| panic!("{} : {issues:?}", file.display()));
            assert_eq!(Some(run.name.as_str()), file.parent().unwrap().file_name().unwrap().to_str());
        }
    }
}
