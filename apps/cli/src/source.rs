//! `dijkstravel source <csv | overpass | atp> runs/<nom> …` → `runs/<nom>/places.geojson`, validé.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use dijkstravel_core::json::to_js_string;
use dijkstravel_core::places::{Place, parse_places, to_places_file};
use dijkstravel_core::run::{Run, RunPaths};
use dijkstravel_sources::{atp, csv, overpass};

use crate::load::{self, Failure, Outcome, fail};

#[derive(Subcommand)]
pub enum Source {
    /// Lieux d'un CSV : colonnes `id`, `name`, `lat`, `lon`, et `source`, `image` facultatives.
    Csv { run: PathBuf, csv: PathBuf },
    /// Lieux d'OpenStreetMap, d'après la section `overpass` du run.
    Overpass { run: PathBuf },
    /// Magasins d'une enseigne en France, d'après un collecteur All The Places.
    Atp {
        run: PathBuf,
        collector: PathBuf,
        /// `store_type` gardés, séparés par des virgules (IKEA : `STORE,SMALL,SHOP`).
        #[arg(long, value_delimiter = ',')]
        types: Option<Vec<String>>,
    },
}

/// Écrit les lieux après la même validation que `validate`.
fn write(run: &Run, paths: &RunPaths, places: &[Place], detail: &str) -> Outcome {
    let text = to_js_string(&to_places_file(places));
    let validated =
        parse_places(&text, &run.bbox).map_err(|issues| Failure(issues.iter().map(ToString::to_string).collect()))?;
    load::write_file(&paths.places, text)?;
    println!("{} : {} lieux{detail}.", paths.places.display(), validated.len());
    Ok(())
}

pub fn main(source: Source) -> Outcome {
    match source {
        Source::Csv { run: arg, csv: file } => {
            let (run, paths) = load::run(&arg)?;
            let places = csv::parse_places_csv(&load::read_text(&file)?)
                .map_err(|issues| Failure(issues.iter().map(ToString::to_string).collect()))?;
            write(&run, &paths, &places, "")
        }
        Source::Overpass { run: arg } => {
            let (run, paths) = load::run(&arg)?;
            let Some(source) = &run.overpass else {
                return fail(format!("{} : aucune section overpass.", paths.file.display()));
            };
            let elements = overpass::parse_response(&overpass::fetch(&overpass::build_query(source, &run.bbox))?)?;
            let found = overpass::places_from_elements(&elements);
            if !found.unnamed.is_empty() {
                eprintln!("{} lieux sans nom écartés : {}", found.unnamed.len(), found.unnamed.join(", "));
            }
            // Un polygone qui chevauche la zone peut avoir son centre en dehors.
            let inside: Vec<Place> = found.places.into_iter().filter(|place| run.bbox.contains(place.at)).collect();
            write(&run, &paths, &inside, &format!(" ({} éléments OSM)", elements.len()))
        }
        Source::Atp { run: arg, collector, types } => atp_source(&arg, &collector, types.as_deref()),
    }
}

fn atp_source(arg: &Path, collector: &Path, types: Option<&[String]>) -> Outcome {
    let (run, paths) = load::run(arg)?;
    let raw = load::read_json(collector)?;
    let places = atp::places_from_collector(&raw, &run.bbox, types);
    let total = raw["features"].as_array().map_or(0, Vec::len);
    write(&run, &paths, &places, &format!(" ({total} dans le fichier)"))
}
