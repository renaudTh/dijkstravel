//! `places.geojson` : le format figé des lieux d'un run, contrat avec toutes les sources.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::{Value, json};

use crate::geo::{BBox, LonLat};
use crate::json::{field, finite, non_empty};

#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub at: LonLat,
    /// D'où vient ce lieu (article, fiche, objet OSM…).
    pub source: Option<String>,
    /// Adresse d'une image du lieu (blason, logo…), téléchargée dans le cache au rendu.
    pub image: Option<String>,
}

impl Place {
    pub fn new(id: impl Into<String>, name: impl Into<String>, at: LonLat) -> Self {
        Place { id: id.into(), name: name.into(), at, source: None, image: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceIssue {
    NotJson,
    NotFeatureCollection,
    Empty,
    NotAPoint { index: usize, id: Option<String> },
    MissingId { index: usize },
    DuplicateId { id: String, indexes: [usize; 2] },
    MissingName { index: usize, id: String },
    InvalidCoordinates { index: usize, id: Option<String> },
    OutsideBBox { index: usize, id: Option<String>, swapped: bool },
}

pub fn parse_places(text: &str, bbox: &BBox) -> Result<Vec<Place>, Vec<PlaceIssue>> {
    let Ok(raw) = serde_json::from_str::<Value>(text) else { return Err(vec![PlaceIssue::NotJson]) };
    let features =
        match (field(&raw, "type").and_then(Value::as_str), field(&raw, "features").and_then(Value::as_array)) {
            (Some("FeatureCollection"), Some(features)) => features,
            _ => return Err(vec![PlaceIssue::NotFeatureCollection]),
        };
    let mut issues = Vec::new();
    let mut places = Vec::new();
    let mut first_index_by_id = HashMap::new();
    for (index, feature) in features.iter().enumerate() {
        let Some(place) = parse_feature(feature, index, bbox, &mut issues) else { continue };
        if let Some(&first) = first_index_by_id.get(&place.id) {
            issues.push(PlaceIssue::DuplicateId { id: place.id, indexes: [first, index] });
            continue;
        }
        first_index_by_id.insert(place.id.clone(), index);
        places.push(place);
    }
    if !issues.is_empty() {
        return Err(issues);
    }
    if places.is_empty() {
        return Err(vec![PlaceIssue::Empty]);
    }
    Ok(places)
}

/// Ajoute les problèmes de la feature à `issues` ; `None` si elle en a.
fn parse_feature(feature: &Value, index: usize, bbox: &BBox, issues: &mut Vec<PlaceIssue>) -> Option<Place> {
    let property = |key: &str| field(feature, "properties").and_then(|properties| field(properties, key));
    let id = property("id").and_then(non_empty).map(str::to_owned);
    let name = property("name").and_then(non_empty);
    let before = issues.len();
    match &id {
        None => issues.push(PlaceIssue::MissingId { index }),
        Some(id) if name.is_none() => issues.push(PlaceIssue::MissingName { index, id: id.clone() }),
        Some(_) => {}
    }
    let geometry = field(feature, "geometry");
    let mut at = None;
    if geometry.and_then(|geometry| field(geometry, "type")).and_then(Value::as_str) == Some("Point") {
        at = geometry.and_then(|geometry| field(geometry, "coordinates")).and_then(parse_coordinates);
        match at {
            None => issues.push(PlaceIssue::InvalidCoordinates { index, id: id.clone() }),
            Some(point) if !bbox.contains(point) => {
                let swapped = bbox.contains([point[1], point[0]]);
                issues.push(PlaceIssue::OutsideBBox { index, id: id.clone(), swapped });
            }
            Some(_) => {}
        }
    } else {
        issues.push(PlaceIssue::NotAPoint { index, id: id.clone() });
    }
    if issues.len() > before {
        return None;
    }
    let text = |key: &str| property(key).and_then(Value::as_str).map(str::to_owned);
    Some(Place {
        id: id?,
        name: name?.trim().to_owned(),
        at: at?,
        source: text("source"),
        image: property("image").and_then(non_empty).map(str::to_owned),
    })
}

/// Au moins deux nombres, dans les bornes des degrés.
fn parse_coordinates(raw: &Value) -> Option<LonLat> {
    let values = raw.as_array()?;
    let (lon, lat) = (finite(values.first()?)?, finite(values.get(1)?)?);
    ((-180.0..=180.0).contains(&lon) && (-90.0..=90.0).contains(&lat)).then_some([lon, lat])
}

#[derive(Serialize)]
struct Properties<'a> {
    id: &'a str,
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<&'a str>,
}

pub fn to_places_file(places: &[Place]) -> Value {
    let features: Vec<Value> = places
        .iter()
        .map(|place| {
            let properties = Properties {
                id: &place.id,
                name: &place.name,
                source: place.source.as_deref(),
                image: place.image.as_deref(),
            };
            json!({ "type": "Feature", "geometry": { "type": "Point", "coordinates": place.at }, "properties": properties })
        })
        .collect();
    json!({ "type": "FeatureCollection", "features": features })
}

fn feature_label(index: usize, id: Option<&str>) -> String {
    match id {
        None => format!("Feature #{index}"),
        Some(id) => format!("Feature #{index} (id \"{id}\")"),
    }
}

impl std::fmt::Display for PlaceIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlaceIssue::NotJson => write!(f, "Le fichier n'est pas du JSON valide."),
            PlaceIssue::NotFeatureCollection => {
                write!(f, "La racine n'est pas un FeatureCollection avec un tableau features.")
            }
            PlaceIssue::Empty => write!(f, "Le fichier ne contient aucun lieu."),
            PlaceIssue::NotAPoint { index, id } => {
                write!(f, "{} : la géométrie doit être un Point.", feature_label(*index, id.as_deref()))
            }
            PlaceIssue::MissingId { index } => write!(f, "{} : id manquant ou vide.", feature_label(*index, None)),
            PlaceIssue::DuplicateId { id, indexes: [a, b] } => {
                write!(f, "id \"{id}\" dupliqué aux features #{a} et #{b}.")
            }
            PlaceIssue::MissingName { index, id } => {
                write!(f, "{} : name manquant ou vide.", feature_label(*index, Some(id)))
            }
            PlaceIssue::InvalidCoordinates { index, id } => write!(
                f,
                "{} : coordonnées invalides, attendu [lon, lat] en degrés.",
                feature_label(*index, id.as_deref())
            ),
            PlaceIssue::OutsideBBox { index, id, swapped } => write!(
                f,
                "{} : hors de la zone du run{}.",
                feature_label(*index, id.as_deref()),
                if *swapped { " (latitude et longitude probablement inversées)" } else { "" }
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRITTANY: BBox = BBox { west: -5.2, south: 47.2, east: -1.0, north: 48.95 };

    fn point(id: Value, name: Value, coordinates: Value) -> Value {
        json!({ "type": "Feature", "geometry": { "type": "Point", "coordinates": coordinates }, "properties": { "id": id, "name": name } })
    }

    fn collection(features: &[Value]) -> String {
        json!({ "type": "FeatureCollection", "features": features }).to_string()
    }

    fn issues(features: &[Value]) -> Vec<PlaceIssue> {
        parse_places(&collection(features), &BRITTANY).unwrap_err()
    }

    fn some(id: &str) -> Option<String> {
        Some(id.to_owned())
    }

    #[test]
    fn accepts_a_conforming_collection_in_file_order() {
        let mut brest = point(json!("b"), json!(" Brest "), json!([-4.49, 48.39]));
        brest["properties"]["source"] = json!("manual");
        brest["properties"]["image"] = json!("https://example.org/brest.svg");
        brest["properties"]["other"] = json!(1);
        let places =
            parse_places(&collection(&[brest, point(json!("a"), json!("Rennes"), json!([-1.68, 48.11]))]), &BRITTANY)
                .unwrap();
        assert_eq!(places[0].name, "Brest");
        assert_eq!(places[0].source.as_deref(), Some("manual"));
        assert_eq!(places[0].image.as_deref(), Some("https://example.org/brest.svg"));
        assert_eq!(places[1], Place::new("a", "Rennes", [-1.68, 48.11]));
    }

    #[test]
    fn round_trips_through_the_file_format() {
        let mut brest = Place::new("b", "Brest", [-4.49, 48.39]);
        brest.image = Some("https://example.org/brest.svg".into());
        let places = vec![brest, Place::new("a", "Rennes", [-1.68, 48.11])];
        let text = to_places_file(&places).to_string();
        assert!(!text.contains("source"));
        assert_eq!(parse_places(&text, &BRITTANY).unwrap(), places);
    }

    #[test]
    fn rejects_files_that_are_not_collections() {
        assert_eq!(parse_places("{\"type\": \"FeatureCollection\",", &BRITTANY), Err(vec![PlaceIssue::NotJson]));
        assert_eq!(parse_places("{\"type\": \"Feature\"}", &BRITTANY), Err(vec![PlaceIssue::NotFeatureCollection]));
        assert_eq!(
            parse_places("{\"type\": \"FeatureCollection\"}", &BRITTANY),
            Err(vec![PlaceIssue::NotFeatureCollection])
        );
        assert_eq!(parse_places(&collection(&[]), &BRITTANY), Err(vec![PlaceIssue::Empty]));
    }

    #[test]
    fn rejects_geometries_that_are_not_points() {
        let mut polygon = point(json!("b"), json!("Brest"), json!([[0, 0]]));
        polygon["geometry"]["type"] = json!("Polygon");
        let mut bare = point(json!("c"), json!("Vannes"), json!([0, 0]));
        bare.as_object_mut().unwrap().remove("geometry");
        assert_eq!(
            issues(&[point(json!("a"), json!("Rennes"), json!([-1.68, 48.11])), polygon, bare]),
            [PlaceIssue::NotAPoint { index: 1, id: some("b") }, PlaceIssue::NotAPoint { index: 2, id: some("c") }]
        );
    }

    #[test]
    fn rejects_missing_empty_or_duplicated_ids() {
        let rennes = || point(json!("a"), json!("Rennes"), json!([-1.68, 48.11]));
        assert_eq!(issues(&[point(json!(""), json!("x"), json!([-2, 48]))]), [PlaceIssue::MissingId { index: 0 }]);
        assert_eq!(issues(&[point(json!(3), json!("x"), json!([-2, 48]))]), [PlaceIssue::MissingId { index: 0 }]);
        assert_eq!(
            issues(&[rennes(), point(json!("b"), json!("Brest"), json!([-4.49, 48.39])), rennes()]),
            [PlaceIssue::DuplicateId { id: "a".into(), indexes: [0, 2] }]
        );
    }

    #[test]
    fn rejects_blank_names() {
        assert_eq!(
            issues(&[point(json!("a"), json!("  "), json!([-1.68, 48.11]))]),
            [PlaceIssue::MissingName { index: 0, id: "a".into() }]
        );
    }

    #[test]
    fn rejects_invalid_coordinates() {
        for coordinates in [json!([200, 48]), json!([-2, 95]), json!(["-2", 48])] {
            assert_eq!(
                issues(&[point(json!("a"), json!("Rennes"), coordinates)]),
                [PlaceIssue::InvalidCoordinates { index: 0, id: some("a") }]
            );
        }
    }

    #[test]
    fn flags_places_outside_the_bbox_and_probable_swaps() {
        assert_eq!(
            issues(&[point(json!("paris"), json!("Paris"), json!([2.35, 48.85]))]),
            [PlaceIssue::OutsideBBox { index: 0, id: some("paris"), swapped: false }]
        );
        assert_eq!(
            issues(&[point(json!("a"), json!("Rennes"), json!([48.11, -1.68]))]),
            [PlaceIssue::OutsideBBox { index: 0, id: some("a"), swapped: true }]
        );
    }

    #[test]
    fn reports_every_faulty_feature() {
        assert_eq!(
            issues(&[
                point(json!("a"), json!(""), json!([-1.68, 48.11])),
                point(json!("b"), json!("Brest"), json!([-4.49, 48.39])),
                point(json!("c"), json!("Vannes"), json!([500, 47.66])),
            ]),
            [
                PlaceIssue::MissingName { index: 0, id: "a".into() },
                PlaceIssue::InvalidCoordinates { index: 2, id: some("c") }
            ]
        );
    }

    #[test]
    fn descriptions_cite_index_and_id() {
        let id = || some("castle-foix");
        for issue in [
            PlaceIssue::NotAPoint { index: 7, id: id() },
            PlaceIssue::MissingName { index: 7, id: "castle-foix".into() },
            PlaceIssue::InvalidCoordinates { index: 7, id: id() },
            PlaceIssue::OutsideBBox { index: 7, id: id(), swapped: false },
        ] {
            let text = issue.to_string();
            assert!(text.contains("castle-foix") && text.contains("#7"), "{text}");
        }
        assert!(PlaceIssue::OutsideBBox { index: 0, id: None, swapped: true }.to_string().contains("inversées"));
    }
}
