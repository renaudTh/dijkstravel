//! Requêtes OSRM et lecture de ses réponses. Le profil de l'URL (`driving`) est ignoré par
//! osrm-routed : il sert le profil de ses données (car, foot…).

use dijkstravel_core::geo::LonLat;
use dijkstravel_core::json::{field, finite, lon_lat};
use serde_json::Value;

use crate::SolveError;

/// Distances en mètres, `None` quand OSRM ne trouve aucune route.
pub type DistanceMatrix = Vec<Vec<Option<f64>>>;

#[derive(Debug, Clone, PartialEq)]
pub struct DistanceTable {
    pub matrix: DistanceMatrix,
    /// Distance entre chaque lieu et le point du réseau où OSRM l'a projeté, en mètres.
    pub snap_meters: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoutedPath {
    pub points: Vec<LonLat>,
    /// Nombre de segments de `points` couverts par chaque étape, dans l'ordre des waypoints.
    pub leg_segments: Vec<usize>,
    /// Traversées en bateau (bac, pirogue…) : indices `[début, fin]` dans `points`, dans l'ordre.
    pub ferry_segments: Vec<[usize; 2]>,
    pub meters: f64,
}

fn osrm(reason: impl Into<String>) -> SolveError {
    SolveError::Osrm(reason.into())
}

fn coordinates_path(points: &[LonLat]) -> String {
    points.iter().map(|[lon, lat]| format!("{lon:.6},{lat:.6}")).collect::<Vec<_>>().join(";")
}

// ponytail: toutes les coordonnées dans l'URL ; découper en blocs sources/destinations si osrm-routed refuse.
pub fn table_url(osrm_url: &str, locations: &[LonLat]) -> String {
    format!("{osrm_url}/table/v1/driving/{}?annotations=distance", coordinates_path(locations))
}

/// `continue_straight=false` : demi-tour permis à chaque lieu, comme dans la matrice ; sinon un lieu au
/// fond d'une impasse ou sur un sens unique rend la tournée entière introuvable (NoRoute).
pub fn route_url(osrm_url: &str, waypoints: &[LonLat]) -> String {
    format!(
        "{osrm_url}/route/v1/driving/{}?overview=full&geometries=geojson&annotations=distance&steps=true&continue_straight=false",
        coordinates_path(waypoints)
    )
}

/// La réponse, si OSRM l'a acceptée (`code: "Ok"`).
pub fn checked(status: u16, raw: Option<Value>) -> Result<Value, SolveError> {
    let Some(raw) = raw.filter(Value::is_object) else { return Err(osrm(format!("HTTP {status} sans corps JSON"))) };
    if raw["code"] != "Ok" {
        let text = |key: &str| raw.get(key).and_then(Value::as_str).unwrap_or("undefined").to_owned();
        return Err(osrm(format!("{} : {}", text("code"), text("message"))));
    }
    Ok(raw)
}

pub fn parse_table(raw: &Value, size: usize) -> Result<DistanceTable, SolveError> {
    let rows = field(raw, "distances")
        .and_then(Value::as_array)
        .filter(|rows| rows.len() == size)
        .ok_or_else(|| osrm("matrice de distances absente ou de mauvaise taille"))?;
    let matrix = rows
        .iter()
        .map(|row| match row.as_array() {
            Some(cells) if cells.len() == size => Ok(cells.iter().map(finite).collect()),
            _ => Err(osrm("ligne de matrice invalide")),
        })
        .collect::<Result<_, _>>()?;
    let sources = field(raw, "sources").and_then(Value::as_array);
    let snap_meters = (0..size)
        .map(|i| {
            sources
                .and_then(|sources| sources.get(i))
                .and_then(|s| field(s, "distance"))
                .and_then(finite)
                .unwrap_or(0.0)
        })
        .collect();
    Ok(DistanceTable { matrix, snap_meters })
}

pub fn parse_route(raw: &Value) -> Result<RoutedPath, SolveError> {
    let route = field(raw, "routes").and_then(|routes| routes.get(0)).ok_or_else(|| osrm("route absente"))?;
    let meters = field(route, "distance").and_then(finite).ok_or_else(|| osrm("route absente"))?;
    let coordinates = field(route, "geometry").and_then(|g| field(g, "coordinates")).and_then(Value::as_array);
    let legs = field(route, "legs").and_then(Value::as_array);
    let (Some(coordinates), Some(legs)) = (coordinates, legs) else {
        return Err(osrm("géométrie ou étapes absentes"));
    };
    let points: Vec<LonLat> = coordinates.iter().filter_map(lon_lat).collect();
    let leg_segments = legs
        .iter()
        .map(|leg| {
            field(leg, "annotation")
                .and_then(|a| field(a, "distance"))
                .and_then(Value::as_array)
                .map(Vec::len)
                .ok_or_else(|| osrm("étape sans annotation de distance"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let segments: usize = leg_segments.iter().sum();
    if points.is_empty() || points.len() != coordinates.len() || segments + 1 != points.len() {
        return Err(osrm(format!("découpage incohérent : {segments} segments pour {} points", points.len())));
    }
    Ok(RoutedPath { ferry_segments: ferry_segments(legs, segments)?, points, leg_segments, meters })
}

/// Traversées en bateau d'après les `steps` (mode `ferry`) : chaque step couvre autant de segments que sa
/// géométrie, qui reprend le dernier point du step précédent, sauf l'arrivée d'une étape (le point
/// d'arrivée deux fois, aucun segment). Sans steps (vieux OSRM, tests) : aucune.
pub fn ferry_segments(legs: &[Value], segments: usize) -> Result<Vec<[usize; 2]>, SolveError> {
    let steps: Vec<&Value> = legs
        .iter()
        .filter_map(|leg| field(leg, "steps").and_then(Value::as_array))
        .flatten()
        .filter(|step| step.is_object())
        .collect();
    if steps.is_empty() {
        return Ok(Vec::new());
    }
    let mut ferries: Vec<[usize; 2]> = Vec::new();
    let mut vertex = 0;
    for step in steps {
        let coordinates = field(step, "geometry")
            .and_then(|g| field(g, "coordinates"))
            .and_then(Value::as_array)
            .ok_or_else(|| osrm("step sans géométrie"))?;
        let arrival = field(step, "maneuver").and_then(|m| field(m, "type")).and_then(Value::as_str) == Some("arrive");
        let end = if arrival { vertex } else { vertex + coordinates.len().saturating_sub(1) };
        if step["mode"] == "ferry" && end > vertex {
            // Deux steps de bateau qui se suivent (escale) : une seule traversée.
            match ferries.last_mut() {
                Some(previous) if previous[1] == vertex => previous[1] = end,
                _ => ferries.push([vertex, end]),
            }
        }
        vertex = end;
    }
    if vertex != segments {
        return Err(osrm(format!("steps incohérents : {vertex} segments pour {segments}")));
    }
    Ok(ferries)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Step OSRM de `points` coordonnées (géométrie factice : seul leur nombre compte).
    fn step(mode: &str, points: usize, kind: &str) -> Value {
        let coordinates: Vec<[usize; 2]> = (0..points).map(|i| [i, 0]).collect();
        json!({ "mode": mode, "maneuver": { "type": kind }, "geometry": { "coordinates": coordinates } })
    }

    fn legs(steps: &[&[Value]]) -> Vec<Value> {
        steps.iter().map(|steps| json!({ "steps": steps })).collect()
    }

    #[test]
    fn finds_no_crossing_on_a_road_only_route() {
        let legs = legs(&[&[step("driving", 4, "depart"), step("driving", 3, "turn"), step("driving", 2, "arrive")]]);
        assert_eq!(ferry_segments(&legs, 5).unwrap(), Vec::<[usize; 2]>::new());
    }

    #[test]
    fn locates_a_ferry_by_the_segments_before_it() {
        let legs = legs(&[&[
            step("driving", 4, "depart"),
            step("ferry", 3, "turn"),
            step("driving", 2, "turn"),
            step("driving", 2, "arrive"),
        ]]);
        assert_eq!(ferry_segments(&legs, 6).unwrap(), [[3, 5]]);
    }

    #[test]
    fn merges_consecutive_ferry_steps() {
        let legs = legs(&[&[
            step("driving", 2, "depart"),
            step("ferry", 3, "turn"),
            step("ferry", 4, "turn"),
            step("driving", 2, "arrive"),
        ]]);
        assert_eq!(ferry_segments(&legs, 6).unwrap(), [[1, 6]]);
    }

    #[test]
    fn counts_no_segment_for_each_arrival() {
        let legs = legs(&[
            &[step("driving", 3, "depart"), step("driving", 2, "arrive")],
            &[step("ferry", 4, "depart"), step("driving", 2, "arrive")],
        ]);
        assert_eq!(ferry_segments(&legs, 5).unwrap(), [[2, 5]]);
    }

    #[test]
    fn finds_no_crossing_without_steps() {
        assert!(ferry_segments(&[json!({ "annotation": { "distance": [1, 2] } })], 2).unwrap().is_empty());
    }

    #[test]
    fn rejects_steps_that_do_not_cover_the_route() {
        let legs = legs(&[&[step("driving", 3, "depart"), step("driving", 2, "arrive")]]);
        assert!(matches!(ferry_segments(&legs, 4), Err(SolveError::Osrm(_))));
    }

    #[test]
    fn reads_a_table_with_missing_routes_and_snaps() {
        let raw = json!({ "code": "Ok", "distances": [[0, null], [3.5, 0]], "sources": [{ "distance": 12.5 }, {}] });
        let table = parse_table(&raw, 2).unwrap();
        assert_eq!(table.matrix, vec![vec![Some(0.0), None], vec![Some(3.5), Some(0.0)]]);
        assert_eq!(table.snap_meters, [12.5, 0.0]);
        assert!(parse_table(&raw, 3).is_err());
    }

    #[test]
    fn reports_the_osrm_error_code() {
        let error = checked(400, Some(json!({ "code": "NoRoute", "message": "Impossible route" }))).unwrap_err();
        assert_eq!(error.to_string(), "OSRM : NoRoute : Impossible route");
        assert!(checked(502, None).is_err());
    }

    #[test]
    fn writes_coordinates_with_six_decimals() {
        assert_eq!(
            table_url("http://o", &[[-1.68, 48.11]]),
            "http://o/table/v1/driving/-1.680000,48.110000?annotations=distance"
        );
    }
}
