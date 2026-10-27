//! Les magasins d'une enseigne en France, d'après All The Places (CC0), souvent plus complet qu'OSM pour
//! les chaînes. Fichier d'un collecteur :
//! `https://alltheplaces-data.openaddresses.io/runs/<run>/output/<collecteur>.geojson` (dernière
//! exécution : `https://data.alltheplaces.xyz/runs/latest.json`).

use dijkstravel_core::geo::BBox;
use dijkstravel_core::json::lon_lat;
use dijkstravel_core::places::Place;
use serde_json::Value;

/// « BELFORT Altkirch » → « Belfort Altkirch » : les enseignes écrivent souvent les villes en capitales.
pub fn tidy(text: &str) -> String {
    let mut out = String::new();
    let mut capitals = String::new();
    let flush = |capitals: &mut String, out: &mut String| {
        let mut chars = capitals.chars();
        if capitals.chars().count() >= 2 {
            out.extend(chars.next());
            out.push_str(&chars.as_str().to_lowercase());
        } else {
            out.push_str(capitals);
        }
        capitals.clear();
    };
    for c in text.chars() {
        if c.is_uppercase() {
            capitals.push(c);
        } else {
            flush(&mut capitals, &mut out);
            out.push(c);
        }
    }
    flush(&mut capitals, &mut out);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Magasins en France dans la zone du run ; `types` : seulement ces `store_type` (IKEA : `STORE`,
/// `SMALL`, `SHOP`, sans les points de retrait).
pub fn places_from_collector(raw: &Value, bbox: &BBox, types: Option<&[String]>) -> Vec<Place> {
    let mut places: Vec<Place> = Vec::new();
    let features = raw["features"].as_array().map(Vec::as_slice).unwrap_or_default();
    for feature in features {
        let props = &feature["properties"];
        let text = |key: &str| props[key].as_str().unwrap_or_default();
        let Some(at) = lon_lat(&feature["geometry"]["coordinates"]) else { continue };
        if text("addr:country").to_uppercase() != "FR" || !bbox.contains(at) {
            continue;
        }
        if types.is_some_and(|types| !types.iter().any(|kind| kind == text("store_type"))) {
            continue;
        }
        let (brand, branch) = (text("name"), tidy(text("branch")));
        let name = if branch.to_lowercase().starts_with(&brand.to_lowercase()) {
            branch
        } else {
            format!("{brand} {branch}").trim().to_owned()
        };
        let reference = props["ref"].as_str().map_or_else(|| format!("{},{}", at[0], at[1]), str::to_owned);
        let spider = props["@spider"].as_str().unwrap_or("atp");
        let mut place = Place::new(format!("atp:{spider}:{reference}"), name, at);
        place.source = props["website"].as_str().map(str::to_owned);
        // Un magasin listé deux fois : le dernier l'emporte, à la place du premier.
        match places.iter_mut().find(|other| other.id == place.id) {
            Some(other) => *other = place,
            None => places.push(place),
        }
    }
    places
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const FRANCE: BBox = BBox { west: -5.3, south: 41.3, east: 9.7, north: 51.2 };

    fn store(reference: &str, branch: &str, kind: &str, at: [f64; 2]) -> Value {
        json!({
            "geometry": { "coordinates": at },
            "properties": { "addr:country": "FR", "name": "Lidl", "branch": branch, "ref": reference, "@spider": "lidl", "store_type": kind }
        })
    }

    #[test]
    fn tidies_capitalised_towns() {
        assert_eq!(tidy("BELFORT  Altkirch "), "Belfort Altkirch");
        assert_eq!(tidy("SAINT-ÉTIENNE A"), "Saint-Étienne A");
    }

    #[test]
    fn keeps_french_stores_in_the_bbox_named_after_their_branch() {
        let mut abroad = store("2", "Basel", "STORE", [7.59, 47.56]);
        abroad["properties"]["addr:country"] = json!("CH");
        let raw = json!({ "features": [store("1", "RENNES Cleunay", "STORE", [-1.7, 48.1]), abroad, store("3", "Lidl Brest", "STORE", [-4.49, 48.39])] });
        let places = places_from_collector(&raw, &FRANCE, None);
        let names: Vec<&str> = places.iter().map(|place| place.name.as_str()).collect();
        assert_eq!(names, ["Lidl Rennes Cleunay", "Lidl Brest"]);
        assert_eq!(places[0].id, "atp:lidl:1");
    }

    #[test]
    fn filters_store_types_and_replaces_duplicates() {
        let raw = json!({ "features": [
            store("1", "A", "STORE", [-1.7, 48.1]),
            store("2", "B", "PAOP", [-1.6, 48.1]),
            store("1", "C", "STORE", [-1.5, 48.1])
        ] });
        let places = places_from_collector(&raw, &FRANCE, Some(&["STORE".to_owned()]));
        assert_eq!(places.len(), 1);
        assert_eq!(places[0].name, "Lidl C");
    }
}
