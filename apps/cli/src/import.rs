//! `dijkstravel import <activité.gpx> runs/<nom>` → un run complet (`run.json`, `places.geojson`,
//! `tour.json`) depuis une activité enregistrée : le tracé tel quel, le départ et l'arrivée nommés d'après
//! leur refuge ou leur commune, les cols et sommets franchis d'après OpenStreetMap, et les heures de
//! passage, que le relief suit. Lit `OVERPASS_URL` (instance publique par défaut).

use std::path::PathBuf;

use dijkstravel_core::geo::{BBox, distance_meters};
use dijkstravel_core::json::to_js_string;
use dijkstravel_core::places::Place;
use dijkstravel_core::run::{Run, TourMode};
use dijkstravel_core::tour::{Marker, MarkerKind, Stop, Tour};
use dijkstravel_sources::gpx::{self, Recorded};
use dijkstravel_sources::{finalize, overpass};
use serde_json::json;

use crate::clock;
use crate::load::{self, Failure, Outcome, RunPaths, fail};

/// Écart minimal entre deux points gardés : le bruit du GPS à l'arrêt disparaît, les lacets restent.
const STEP_METERS: f64 = 5.0;
/// Descente qui sépare deux sommets du profil.
const SUMMIT_DROP_METERS: f64 = 150.0;
/// Distance au plus entre le point le plus haut du tracé et le col ou sommet d'OpenStreetMap qu'il nomme.
const SUMMIT_RADIUS_METERS: f64 = 200.0;

#[derive(clap::Args)]
pub struct Import {
    /// Fichier GPX de l'activité (export Strava de sa propre activité : avec les heures).
    pub file: PathBuf,
    /// `runs/<nom>` à créer ; le nom du dossier est celui du run.
    pub run: PathBuf,
    /// Titre de la carte d'infos, à la place du nom de la trace.
    #[arg(long)]
    pub title: Option<String>,
    /// Thème (`themes/<nom>.json`) ; par défaut `trail` pour une activité à pied, sinon `default`.
    #[arg(long)]
    pub theme: Option<String>,
    /// Format de la vidéo : `16:9` ou `9:16`.
    #[arg(long, default_value = "16:9", value_parser = ["16:9", "9:16"])]
    pub format: String,
    /// Sans OpenStreetMap : départ et arrivée sans nom, aucun col ni sommet.
    #[arg(long)]
    pub offline: bool,
    /// Durée du trajet dans la vidéo, en secondes.
    #[arg(long, default_value_t = 60.0)]
    pub duration: f64,
}

pub fn main(import: Import) -> Outcome {
    let paths = RunPaths::of(&import.run, "");
    if paths.file.exists() {
        return fail(format!(
            "{} existe déjà : choisir un autre dossier, ou le supprimer d'abord.",
            paths.file.display()
        ));
    }
    let name = paths
        .dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| Failure(vec![format!("{} : dossier du run sans nom", import.run.display())]))?
        .to_owned();
    let activity = gpx::parse(&load::read_text(&import.file)?)?;
    let recorded = activity.recorded(STEP_METERS);
    let path = &recorded.path;
    let (start, finish) = (path.points[0], path.points[path.points.len() - 1]);
    let bbox = bbox_of(&recorded);

    let url = std::env::var("OVERPASS_URL").unwrap_or_else(|_| overpass::OVERPASS_URL.to_owned());
    let place_name = |at, fallback: &str| {
        let found = if import.offline {
            Ok(None)
        } else {
            fetch(&url, &overpass::place_name_query(at)).map(|raw| overpass::parse_place_name(&raw))
        };
        match found {
            Ok(Some(name)) => name,
            Ok(None) => fallback.to_owned(),
            Err(error) => {
                eprintln!("{fallback} sans nom ({error}) : à renommer dans places.geojson et tour.json.");
                fallback.to_owned()
            }
        }
    };
    let start_name = place_name(start, "Départ");
    let finish_name = place_name(finish, "Arrivée");
    let summits = if import.offline {
        Ok(Vec::new())
    } else {
        fetch(&url, &overpass::summits_query(&bbox)).and_then(|raw| overpass::parse_summits(&raw))
    };
    let markers = match summits {
        Ok(summits) => climbs(&recorded, &summits),
        Err(error) => {
            eprintln!("Cols et sommets introuvables ({error}) : run sans repères.");
            Vec::new()
        }
    };

    let schedule = match (recorded.timeline.first(), recorded.timeline.last()) {
        (Some(&[_, departure]), Some(&[_, arrival])) => Some(json!({
            "start": clock::local(departure, start)?.to_string(),
            "finish": clock::local(arrival, finish)?.to_string(),
        })),
        _ => {
            eprintln!(
                "{} : aucune heure dans le fichier (activité d'un autre exportée par Strava ?) : ajouter `schedule` à la main.",
                import.file.display()
            );
            None
        }
    };

    let depart = format!("{name}-depart");
    let arrivee = format!("{name}-arrivee");
    let file_name = import.file.file_name().map_or_else(String::new, |file| file.to_string_lossy().into_owned());
    let title = import.title.clone().or_else(|| activity.name.clone()).unwrap_or_else(|| name.clone());
    let sport = activity.sport.as_deref().unwrap_or("");
    let on_foot = ["run", "trail", "hik", "walk", "foot"].iter().any(|kind| sport.contains(kind));
    let mut run = json!({
        "name": name,
        "title": title,
        "source": {
            "kind": "gpx",
            "description": format!("Activité enregistrée « {} »{} ({file_name})", activity.name.as_deref().unwrap_or(&name), if sport.is_empty() { String::new() } else { format!(", {sport}") }),
            "date": jiff::Zoned::now().date().to_string(),
            "licence": "Activité personnelle, usage privé",
        },
        "bbox": { "west": bbox.west, "south": bbox.south, "east": bbox.east, "north": bbox.north },
        "start": { "place": depart },
        "mode": "open",
        "profile": if on_foot { "foot" } else { "car" },
        "theme": import.theme.clone().unwrap_or_else(|| (if on_foot { "trail" } else { "default" }).to_owned()),
        "video": { "format": import.format, "durationSec": import.duration, "introSec": 2, "outroSec": 4 },
    });
    if let Some(schedule) = schedule {
        run["schedule"] = schedule;
    }
    let parsed = Run::parse(&run)
        .map_err(|issues| Failure(issues.iter().map(|issue| format!("run.json : {issue}")).collect()))?;

    let source = format!("gpx:{file_name}");
    let places: Vec<Place> = [(&depart, &start_name, start), (&arrivee, &finish_name, finish)]
        .into_iter()
        .map(|(id, name, at)| Place { source: Some(source.clone()), ..Place::new(id.as_str(), name.as_str(), at) })
        .collect();
    let (places_text, _) = finalize(&places, &parsed.bbox)
        .map_err(|issues| Failure(issues.iter().map(|issue| format!("places.geojson : {issue}")).collect()))?;
    let total = path.total_meters();
    let tour = Tour {
        run: parsed.name.clone(),
        mode: TourMode::Open,
        start,
        path: path.clone(),
        stops: vec![
            Stop { id: depart.clone(), name: start_name.clone(), at: start, meters: 0.0 },
            Stop { id: arrivee.clone(), name: finish_name.clone(), at: finish, meters: total },
        ],
        excluded: Vec::new(),
        ferries: Vec::new(),
        transfers: Vec::new(),
        stages: Vec::new(),
        markers,
        timeline: recorded.timeline.clone(),
        routed_meters: total,
    };

    load::write_file(&paths.file, format!("{}\n", serde_json::to_string_pretty(&run)?))?;
    load::write_file(&paths.places, places_text)?;
    load::write_file(&paths.tour, to_js_string(&tour))?;
    println!("{} : {:.1} km, de {start_name} à {finish_name}.", paths.dir.display(), total / 1000.0);
    if let (Some(&[_, departure]), Some(&[_, arrival])) = (tour.timeline.first(), tour.timeline.last()) {
        let hours = (arrival - departure) / 3600.0;
        println!(
            "Heures de passage : {} → {} ({}h{:02}).",
            run["schedule"]["start"].as_str().unwrap_or(""),
            run["schedule"]["finish"].as_str().unwrap_or(""),
            hours.floor(),
            ((hours.fract()) * 60.0).round()
        );
    }
    for marker in &tour.markers {
        println!("  km {:5.1}  {}", marker.meters / 1000.0, marker.name);
    }
    Ok(())
}

/// Une requête Overpass, réessayée deux fois : l'instance publique répond souvent 502 ou 504 un moment.
fn fetch(url: &str, query: &str) -> Result<serde_json::Value, String> {
    let mut attempt = 1;
    loop {
        match overpass::fetch(url, query) {
            Ok(raw) => return Ok(raw),
            Err(error) if attempt >= 3 => return Err(error),
            Err(error) => {
                eprintln!("{error} : nouvel essai dans {} s.", 10 * attempt);
                std::thread::sleep(std::time::Duration::from_secs(10 * attempt));
                attempt += 1;
            }
        }
    }
}

/// Le cadre du tracé, avec une marge d'environ 500 m.
fn bbox_of(recorded: &Recorded) -> BBox {
    let points = &recorded.path.points;
    let fold = |pick: fn(&[f64; 2]) -> f64, min: bool| {
        points
            .iter()
            .map(pick)
            .fold(if min { f64::INFINITY } else { f64::NEG_INFINITY }, |a, b| if min { a.min(b) } else { a.max(b) })
    };
    let margin = 0.005;
    // Au millionième de degré (10 cm), comme les points.
    let round = |value: f64| (value * 1e6).round() / 1e6;
    BBox {
        west: round(fold(|p| p[0], true) - margin),
        south: round(fold(|p| p[1], true) - margin),
        east: round(fold(|p| p[0], false) + margin),
        north: round(fold(|p| p[1], false) + margin),
    }
}

/// Les sommets du profil nommés par le col ou sommet d'OpenStreetMap le plus proche, chacun une fois.
fn climbs(recorded: &Recorded, summits: &[overpass::Summit]) -> Vec<Marker> {
    let mut markers: Vec<Marker> = Vec::new();
    for index in gpx::summits(&recorded.elevations, SUMMIT_DROP_METERS) {
        let at = recorded.path.points[index];
        let nearest = summits
            .iter()
            .map(|summit| (distance_meters(at, summit.at), summit))
            .filter(|(distance, _)| *distance <= SUMMIT_RADIUS_METERS)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, summit)) = nearest else { continue };
        let elevation = summit.elevation.unwrap_or(recorded.elevations[index]).round();
        let name = format!("{} · {elevation} m", summit.name);
        if markers.iter().any(|marker| marker.name == name) {
            continue;
        }
        markers.push(Marker { kind: MarkerKind::Climb, name, category: None, at, meters: recorded.path.meters[index] });
    }
    markers
}
