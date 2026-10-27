//! Lieux d'OpenStreetMap par Overpass : la section `overpass` du run (zone ISO et filtre de tags).

use dijkstravel_core::geo::{BBox, LonLat, distance_meters};
use dijkstravel_core::json::finite;
use dijkstravel_core::places::Place;
use dijkstravel_core::run::OverpassSource;
use serde_json::Value;

pub const OVERPASS_URL: &str = "https://overpass-api.de/api/interpreter";

pub const MERGE_RADIUS_METERS: f64 = 50.0;

/// Élément d'une réponse `out center` : les ways et relations n'ont que leur centre.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// `node`, `way` ou `relation`.
    pub kind: String,
    pub id: i64,
    pub at: LonLat,
    pub name: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct OverpassPlaces {
    pub places: Vec<Place>,
    /// `id` des lieux écartés faute de tag `name`.
    pub unnamed: Vec<String>,
}

pub fn build_query(source: &OverpassSource, bbox: &BBox) -> String {
    let iso_key = if source.area_iso.contains('-') { "ISO3166-2" } else { "ISO3166-1" };
    [
        "[out:json][timeout:300];".to_owned(),
        format!("area[\"{iso_key}\"=\"{}\"]->.zone;", source.area_iso),
        format!("nwr{}(area.zone)({},{},{},{});", source.filter, bbox.south, bbox.west, bbox.north, bbox.east),
        "out center tags;".to_owned(),
    ]
    .join("\n")
}

pub fn parse_response(raw: &Value) -> Result<Vec<Element>, String> {
    let elements = raw["elements"].as_array().ok_or("Overpass : réponse sans tableau elements")?;
    elements
        .iter()
        .map(|element| {
            let id = element["id"].as_i64().ok_or("Overpass : élément sans id")?;
            let kind = element["type"].as_str().unwrap_or_default();
            let position = if kind == "node" { element } else { &element["center"] };
            let at = match (finite(&position["lon"]), finite(&position["lat"])) {
                (Some(lon), Some(lat)) if matches!(kind, "node" | "way" | "relation") => [lon, lat],
                _ => return Err(format!("Overpass : élément {kind}/{id} sans position")),
            };
            let name = element["tags"]["name"].as_str().map(str::to_owned);
            Ok(Element { kind: kind.to_owned(), id, at, name })
        })
        .collect()
}

/// Les éléments à moins de 50 m l'un de l'autre ne font qu'un lieu, nommé si l'un d'eux l'est.
pub fn places_from_elements(elements: &[Element]) -> OverpassPlaces {
    let mut kept: Vec<&Element> = Vec::new();
    // ponytail: fusion en O(n²), passer à une grille au-delà de quelques milliers de lieux.
    for element in elements {
        match kept.iter().position(|other| distance_meters(other.at, element.at) < MERGE_RADIUS_METERS) {
            None => kept.push(element),
            Some(twin) if kept[twin].name.is_none() && element.name.is_some() => kept[twin] = element,
            Some(_) => {}
        }
    }
    let mut result = OverpassPlaces::default();
    for element in kept {
        let id = format!("osm:{}/{}", element.kind, element.id);
        match element.name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
            None => result.unnamed.push(id),
            Some(name) => {
                let mut place = Place::new(id, name, element.at);
                place.source = Some(format!("https://www.openstreetmap.org/{}/{}", element.kind, element.id));
                result.places.push(place);
            }
        }
    }
    result
}

pub fn fetch(query: &str) -> Result<Value, String> {
    let mut response = ureq::post(OVERPASS_URL)
        .header("user-agent", "dijkstravel")
        .send_form([("data", query)])
        .map_err(|e| format!("Overpass : {e}"))?;
    response.body_mut().with_config().limit(1 << 30).read_json().map_err(|e| format!("Overpass : {e}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // 0.0003° de latitude ≈ 33 m, 0.00055° ≈ 61 m.
    const NEAR: f64 = 0.0003;
    const FAR: f64 = 0.00055;

    fn node(id: i64, lat: f64, lon: f64, name: Option<&str>) -> Element {
        Element { kind: "node".into(), id, at: [lon, lat], name: name.map(str::to_owned) }
    }

    fn ids(result: &OverpassPlaces) -> Vec<&str> {
        result.places.iter().map(|place| place.id.as_str()).collect()
    }

    #[test]
    fn reads_nodes_ways_and_relations_by_their_position() {
        let raw = json!({ "elements": [
            { "type": "node", "id": 1, "lat": 48.11, "lon": -1.68, "tags": { "name": "Rennes" } },
            { "type": "way", "id": 2, "center": { "lat": 48.39, "lon": -4.49 }, "tags": { "name": "Brest" } },
            { "type": "relation", "id": 3, "center": { "lat": 47.66, "lon": -2.76 }, "tags": { "name": "Vannes" } }
        ] });
        let result = places_from_elements(&parse_response(&raw).unwrap());
        assert_eq!(ids(&result), ["osm:node/1", "osm:way/2", "osm:relation/3"]);
        assert_eq!(result.places[1].at, [-4.49, 48.39]);
        assert_eq!(result.places[2].source.as_deref(), Some("https://www.openstreetmap.org/relation/3"));
    }

    #[test]
    fn rejects_an_element_without_position() {
        assert!(parse_response(&json!({ "elements": [{ "type": "way", "id": 2, "tags": {} }] })).is_err());
        assert!(parse_response(&json!({})).is_err());
    }

    #[test]
    fn merges_places_closer_than_fifty_meters_keeping_the_named_one() {
        let merged = places_from_elements(&[
            node(1, 48.11, -1.68, Some("Château")),
            node(2, 48.11 + NEAR, -1.68, Some("Entrée")),
        ]);
        assert_eq!(ids(&merged), ["osm:node/1"]);
        let named = places_from_elements(&[node(1, 48.11, -1.68, None), node(2, 48.11 + NEAR, -1.68, Some("Château"))]);
        assert_eq!(ids(&named), ["osm:node/2"]);
        let apart = places_from_elements(&[
            node(1, 48.11, -1.68, Some("Château")),
            node(2, 48.11 + FAR, -1.68, Some("Manoir")),
        ]);
        assert_eq!(apart.places.len(), 2);
    }

    #[test]
    fn drops_and_lists_places_without_a_name() {
        let result = places_from_elements(&[node(1, 48.11, -1.68, Some("Rennes")), node(2, 48.39, -4.49, Some("  "))]);
        assert_eq!(ids(&result), ["osm:node/1"]);
        assert_eq!(result.unnamed, ["osm:node/2"]);
    }

    #[test]
    fn builds_the_query_for_a_country_or_a_region() {
        let bbox = BBox { west: -5.25, south: 47.2, east: -1.0, north: 48.95 };
        let source = OverpassSource { area_iso: "FR-BRE".into(), filter: "[\"historic\"=\"castle\"]".into() };
        let query = build_query(&source, &bbox);
        assert!(query.contains("area[\"ISO3166-2\"=\"FR-BRE\"]->.zone;"));
        assert!(query.contains("nwr[\"historic\"=\"castle\"](area.zone)(47.2,-5.25,48.95,-1);"));
    }
}
