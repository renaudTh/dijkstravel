//! Contexte Sourcing : les lieux d'un run, traduits depuis chaque source externe (couches
//! anti-corruption CSV, Overpass, All The Places), vers `places.geojson`. Une source ponctuelle reste un script
//! jetable qui produit un CSV ou un GeoJSON : il passe par `csv`, puis le résultat est versionné avec sa
//! provenance dans `run.json`.

pub mod atp;
pub mod csv;
pub mod gpx;
pub mod overpass;

use dijkstravel_core::geo::BBox;
use dijkstravel_core::json::to_js_string;
use dijkstravel_core::places::{Place, PlaceIssue, parse_places, to_places_file};

/// Le contenu de `places.geojson` pour `places`, et les lieux tels qu'ils seront relus : la même
/// validation que celle d'un fichier écrit à la main, jamais de lieux enregistrés tels quels.
pub fn finalize(places: &[Place], bbox: &BBox) -> Result<(String, Vec<Place>), Vec<PlaceIssue>> {
    let text = to_js_string(&to_places_file(places));
    let validated = parse_places(&text, bbox)?;
    Ok((text, validated))
}
