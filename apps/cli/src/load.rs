//! Lecture des fichiers d'un run, chacun validé : un problème est rapporté avec son fichier et son champ.

use std::path::{Path, PathBuf};

use dijkstravel_core::places::{Place, parse_places};
use dijkstravel_core::run::Run;
use dijkstravel_core::theme::Theme;
use dijkstravel_core::tour::Tour;
use serde_json::Value;

/// Échec d'une commande, déjà décrit pour l'utilisateur, une ligne par problème.
pub struct Failure(pub Vec<String>);

impl<E: std::fmt::Display> From<E> for Failure {
    fn from(error: E) -> Self {
        Failure(vec![error.to_string()])
    }
}

pub type Outcome<T = ()> = Result<T, Failure>;

pub fn fail<T>(message: impl Into<String>) -> Outcome<T> {
    Err(Failure(vec![message.into()]))
}

pub fn read_text(file: &Path) -> Outcome<String> {
    std::fs::read_to_string(file).map_err(|e| Failure(vec![format!("{} : {e}", file.display())]))
}

pub fn read_json(file: &Path) -> Outcome<Value> {
    serde_json::from_str(&read_text(file)?).map_err(|e| Failure(vec![format!("{} : {e}", file.display())]))
}

/// Crée le dossier parent au besoin ; écrit à côté puis renomme, pour ne jamais laisser un fichier à moitié écrit.
pub fn write_file(file: &Path, contents: impl AsRef<[u8]>) -> Outcome {
    if let Some(dir) = file.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let partial = PathBuf::from(format!("{}.{}.part", file.display(), std::process::id()));
    std::fs::write(&partial, contents).map_err(|e| format!("{} : {e}", partial.display()))?;
    std::fs::rename(&partial, file)?;
    Ok(())
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

/// `runs/<nom>` ou `runs/<nom>/run.json`.
pub fn run(arg: &Path) -> Outcome<(Run, RunPaths)> {
    let file = run_file(arg);
    let run = Run::parse(&read_json(&file)?)
        .map_err(|issues| Failure(issues.iter().map(|issue| format!("{} : {issue}", file.display())).collect()))?;
    let paths = RunPaths::of(arg, &run.name);
    Ok((run, paths))
}

pub fn places(run: &Run, paths: &RunPaths) -> Outcome<Vec<Place>> {
    let text = read_text(&paths.places)?;
    parse_places(&text, &run.bbox)
        .map_err(|issues| Failure(issues.iter().map(|issue| format!("{} : {issue}", paths.places.display())).collect()))
}

pub fn tour(paths: &RunPaths) -> Outcome<Tour> {
    Tour::parse(&read_text(&paths.tour)?).map_err(|e| Failure(vec![format!("{} : {e}", paths.tour.display())]))
}

/// Fichier du thème du run ; absent, le thème par défaut.
pub fn theme_file(run: &Run) -> PathBuf {
    PathBuf::from(format!("themes/{}.json", run.theme))
}

pub fn theme(run: &Run) -> Outcome<Theme> {
    let file = theme_file(run);
    let raw = if file.exists() { read_json(&file)? } else { Value::Object(serde_json::Map::new()) };
    Theme::parse(&raw)
        .map_err(|issues| Failure(issues.iter().map(|issue| format!("{} : {issue}", file.display())).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locates_files_from_the_run_folder() {
        let from_dir = RunPaths::of("runs/bretagne", "bretagne");
        assert_eq!(from_dir, RunPaths::of("runs/bretagne/run.json", "bretagne"));
        assert_eq!(from_dir.places, Path::new("runs/bretagne/places.geojson"));
        assert_eq!(from_dir.tour, Path::new("runs/bretagne/tour.json"));
        assert_eq!(from_dir.video, Path::new("data/out/bretagne/video.mp4"));
    }
}
