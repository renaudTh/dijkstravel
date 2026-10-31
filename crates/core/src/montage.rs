//! `montage.json` : une vidéo faite de plusieurs runs déjà calculés, l'un après l'autre. Chaque segment
//! garde sa tournée et sa caméra ; entre deux, la caméra vole d'une ville à l'autre, sans trajet ni
//! distance. Le vol remplace l'intro et l'outro de chaque segment, à 0 par défaut.

use serde_json::Value;

use crate::Issue;
use crate::json::{field, finite, non_empty};
use crate::run::{Run, VideoFormat, VideoSettings};
use crate::theme::is_theme_name;

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// Le nom du run, dossier voisin de celui du montage.
    pub run: String,
    /// Remplace le titre du run sur la carte d'infos.
    pub title: Option<String>,
    /// Nom affiché en vue d'avion, au début du segment.
    pub city: Option<String>,
    pub duration_sec: f64,
    pub intro_sec: f64,
    pub outro_sec: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Montage {
    pub name: String,
    pub title: Option<String>,
    /// Un seul thème pour tous les segments.
    pub theme: String,
    pub format: VideoFormat,
    pub fps: u32,
    /// Durée de chaque vol entre deux segments.
    pub transition_sec: f64,
    pub segments: Vec<Segment>,
}

const SEGMENT_INTRO_SEC: f64 = 0.0;
const SEGMENT_OUTRO_SEC: f64 = 0.0;
const TRANSITION_SEC: f64 = 4.0;

impl Montage {
    pub fn parse(raw: &Value) -> Result<Montage, Vec<Issue>> {
        if !raw.is_object() {
            return Err(vec![Issue::new("", "objet")]);
        }
        let mut issues = Vec::new();
        let get = |key: &str| field(raw, key);
        let name = get("name").and_then(non_empty).filter(|name| is_folder_name(name));
        if name.is_none() {
            issues.push(Issue::new("name", "nom de dossier"));
        }
        let theme = match get("theme") {
            None => Some("default"),
            Some(value) => value.as_str().filter(|name| is_theme_name(name)),
        };
        if theme.is_none() {
            issues.push(Issue::new("theme", "nom de fichier (lettres, chiffres, - et _)"));
        }
        let empty = Value::Object(serde_json::Map::new());
        let video = get("video").unwrap_or(&empty);
        let format = match field(video, "format").map(Value::as_str) {
            None | Some(Some("16:9")) => VideoFormat::Landscape,
            Some(Some("9:16")) => VideoFormat::Portrait,
            Some(_) => {
                issues.push(Issue::new("video.format", "'16:9' | '9:16'"));
                VideoFormat::Landscape
            }
        };
        let fps = number(video, "fps", f64::from(VideoSettings::default().fps), "video.fps", &mut issues, |n| {
            n > 0.0 && n.fract() == 0.0
        });
        let transition_sec =
            number(video, "transitionSec", TRANSITION_SEC, "video.transitionSec", &mut issues, |n| n >= 0.0);

        let mut segments = Vec::new();
        match get("segments").and_then(Value::as_array).filter(|list| !list.is_empty()) {
            None => issues.push(Issue::new("segments", "liste non vide")),
            Some(list) => {
                for (i, raw) in list.iter().enumerate() {
                    let path = |key: &str| format!("segments[{i}].{key}");
                    let run = field(raw, "run").and_then(non_empty).filter(|name| is_folder_name(name));
                    if run.is_none() {
                        issues.push(Issue::new(path("run"), "nom de run"));
                    }
                    let duration_sec = match field(raw, "durationSec").and_then(finite).filter(|&n| n > 0.0) {
                        Some(n) => n,
                        None => {
                            issues.push(Issue::new(path("durationSec"), "nombre > 0"));
                            1.0
                        }
                    };
                    let at_least_zero = |n: f64| n >= 0.0;
                    let intro_sec =
                        number(raw, "introSec", SEGMENT_INTRO_SEC, &path("introSec"), &mut issues, at_least_zero);
                    let outro_sec =
                        number(raw, "outroSec", SEGMENT_OUTRO_SEC, &path("outroSec"), &mut issues, at_least_zero);
                    segments.push(Segment {
                        run: run.unwrap_or_default().to_owned(),
                        title: field(raw, "title").and_then(non_empty).map(|title| title.trim().to_owned()),
                        city: field(raw, "city").and_then(non_empty).map(|city| city.trim().to_owned()),
                        duration_sec,
                        intro_sec,
                        outro_sec,
                    });
                }
            }
        }

        match (name, theme) {
            (Some(name), Some(theme)) if issues.is_empty() => Ok(Montage {
                name: name.to_owned(),
                title: get("title").and_then(non_empty).map(|title| title.trim().to_owned()),
                theme: theme.to_owned(),
                format,
                fps: fps as u32,
                transition_sec,
                segments,
            }),
            _ => Err(issues),
        }
    }

    /// Le run du segment tel que le montage le rend : son format, son thème, ses durées, son titre.
    pub fn segment_run(&self, segment: &Segment, mut run: Run) -> Run {
        run.theme = self.theme.clone();
        run.video = VideoSettings {
            format: self.format,
            duration_sec: segment.duration_sec,
            intro_sec: segment.intro_sec,
            outro_sec: segment.outro_sec,
            fps: self.fps,
            markers: run.video.markers,
        };
        if segment.title.is_some() {
            run.title = segment.title.clone();
        }
        run
    }
}

fn is_folder_name(name: &str) -> bool {
    name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// Champ numérique facultatif : `fallback` s'il est absent, un problème s'il est invalide.
fn number(
    raw: &Value,
    key: &str,
    fallback: f64,
    path: &str,
    issues: &mut Vec<Issue>,
    valid: impl Fn(f64) -> bool,
) -> f64 {
    match field(raw, key) {
        None => fallback,
        Some(value) => finite(value).filter(|&n| valid(n)).unwrap_or_else(|| {
            issues.push(Issue::new(path, if key == "fps" { "entier > 0" } else { "nombre >= 0" }));
            fallback
        }),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn montage() -> Value {
        json!({
            "name": "manifs",
            "theme": "rouge-noir",
            "video": { "format": "9:16", "transitionSec": 2.5 },
            "segments": [
                { "run": "lyon", "title": "Lyon · 14 h", "city": "Lyon", "durationSec": 6 },
                { "run": "nice", "durationSec": 5, "introSec": 0.5, "outroSec": 0 }
            ]
        })
    }

    #[test]
    fn reads_segments_with_their_defaults() {
        let montage = Montage::parse(&montage()).unwrap();
        assert_eq!((montage.format, montage.fps, montage.transition_sec), (VideoFormat::Portrait, 30, 2.5));
        assert_eq!(montage.segments[0].title.as_deref(), Some("Lyon · 14 h"));
        assert_eq!(montage.segments[0].city.as_deref(), Some("Lyon"));
        assert_eq!((montage.segments[0].intro_sec, montage.segments[0].outro_sec), (0.0, 0.0));
        assert_eq!((montage.segments[1].intro_sec, montage.segments[1].outro_sec), (0.5, 0.0));
    }

    #[test]
    fn reports_every_invalid_field() {
        let mut raw = montage();
        raw["segments"][0]["durationSec"] = json!(0);
        raw["segments"][1]["run"] = json!("../x y");
        raw["video"]["transitionSec"] = json!(-1);
        let paths: Vec<String> = Montage::parse(&raw).unwrap_err().into_iter().map(|issue| issue.path).collect();
        assert_eq!(paths, ["video.transitionSec", "segments[0].durationSec", "segments[1].run"]);
        assert!(Montage::parse(&json!({ "name": "m", "segments": [] })).is_err());
    }

    #[test]
    fn every_montage_in_the_repository_is_valid_and_its_runs_exist() {
        let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../runs"));
        for entry in std::fs::read_dir(dir).unwrap() {
            let file = entry.unwrap().path().join("montage.json");
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            let montage = Montage::parse(&serde_json::from_str(&text).unwrap())
                .unwrap_or_else(|issues| panic!("{} : {issues:?}", file.display()));
            assert_eq!(Some(montage.name.as_str()), file.parent().unwrap().file_name().unwrap().to_str());
            for segment in &montage.segments {
                assert!(dir.join(&segment.run).join("run.json").exists(), "{} : run {}", file.display(), segment.run);
            }
        }
    }
}
