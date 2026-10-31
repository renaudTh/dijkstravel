//! Requête VROOM sur matrice fournie et lecture de l'ordre de visite.

use dijkstravel_core::run::TourMode;
use serde_json::{Value, json};

use crate::SolveError;
use crate::osrm::DistanceMatrix;

/// Départ résolu en indice de matrice. `None` : départ libre en trajet ouvert, modélisé par un point
/// fictif à distance nulle de tous les lieux.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Departure {
    Index(usize),
    None,
}

fn vroom(reason: impl Into<String>) -> SolveError {
    SolveError::Vroom(reason.into())
}

/// `matrix` couvre les `job_count` lieux, puis éventuellement le point de départ libre.
pub fn build_request(
    matrix: &DistanceMatrix,
    job_count: usize,
    departure: Departure,
    mode: TourMode,
) -> Result<Value, SolveError> {
    let size = matrix.len();
    let mut durations = matrix
        .iter()
        .enumerate()
        .map(|(from, row)| {
            row.iter()
                .enumerate()
                .map(|(to, cell)| {
                    cell.map(|meters| meters.round() as i64)
                        .ok_or_else(|| vroom(format!("aucune route de {from} vers {to}")))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    // VROOM minimise `durations` par défaut : on lui donne des mètres pour obtenir le plus court.
    let start_index = match departure {
        Departure::Index(index) => index,
        Departure::None => {
            for row in &mut durations {
                row.push(0);
            }
            durations.push(vec![0; size + 1]);
            size
        }
    };
    let vehicle = match mode {
        TourMode::Loop => json!({ "id": 0, "profile": "car", "start_index": start_index, "end_index": start_index }),
        TourMode::Open | TourMode::Ordered => json!({ "id": 0, "profile": "car", "start_index": start_index }),
    };
    let jobs: Vec<Value> = (0..job_count).map(|index| json!({ "id": index, "location_index": index })).collect();
    Ok(json!({ "vehicles": [vehicle], "jobs": jobs, "matrices": { "car": { "durations": durations } } }))
}

/// Indices des lieux dans l'ordre de visite.
pub fn parse_solution(raw: &Value, job_count: usize) -> Result<Vec<usize>, SolveError> {
    if !raw.is_object() {
        return Err(vroom("réponse illisible"));
    }
    if raw["code"] != 0 {
        return Err(vroom(format!("code {} : {}", raw["code"], raw["error"])));
    }
    if let Some(unassigned) = raw["unassigned"].as_array().filter(|list| !list.is_empty()) {
        return Err(vroom(format!("{} lieux non assignés", unassigned.len())));
    }
    let steps = raw["routes"][0]["steps"].as_array().ok_or_else(|| vroom("aucune route dans la solution"))?;
    let order: Vec<usize> = steps
        .iter()
        .filter(|step| step["type"] == "job")
        .filter_map(|step| step["id"].as_u64())
        .map(|id| id as usize)
        .collect();
    let mut distinct = order.clone();
    distinct.sort_unstable();
    distinct.dedup();
    if order.len() != job_count || distinct.len() != job_count {
        return Err(vroom(format!("{} lieux visités sur {job_count}", order.len())));
    }
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix(rows: &[&[f64]]) -> DistanceMatrix {
        rows.iter().map(|row| row.iter().map(|&cell| Some(cell)).collect()).collect()
    }

    fn three_places() -> DistanceMatrix {
        matrix(&[&[0.0, 5.0, 7.0], &[5.0, 0.0, 4.0], &[7.0, 4.0, 0.0]])
    }

    // Trois lieux, puis un départ en coordonnées à l'indice 3.
    fn three_places_and_start() -> DistanceMatrix {
        matrix(&[&[0.0, 5.0, 7.0, 2.0], &[5.0, 0.0, 4.0, 3.0], &[7.0, 4.0, 0.0, 6.0], &[2.0, 3.0, 6.0, 0.0]])
    }

    #[test]
    fn creates_one_job_per_place() {
        let request = build_request(&three_places(), 3, Departure::Index(0), TourMode::Loop).unwrap();
        assert_eq!(
            request["jobs"],
            json!([{ "id": 0, "location_index": 0 }, { "id": 1, "location_index": 1 }, { "id": 2, "location_index": 2 }])
        );
        let single =
            build_request(&matrix(&[&[0.0, 9.0], &[9.0, 0.0]]), 1, Departure::Index(1), TourMode::Loop).unwrap();
        assert_eq!(single["jobs"], json!([{ "id": 0, "location_index": 0 }]));
    }

    #[test]
    fn ends_the_vehicle_at_its_departure_only_in_loop_mode() {
        let looped = build_request(&three_places_and_start(), 3, Departure::Index(3), TourMode::Loop).unwrap();
        assert_eq!(looped["vehicles"][0]["end_index"], 3);
        let open = build_request(&three_places_and_start(), 3, Departure::Index(3), TourMode::Open).unwrap();
        assert_eq!(open["vehicles"][0]["start_index"], 3);
        assert!(open["vehicles"][0].get("end_index").is_none());
    }

    #[test]
    fn starts_a_free_open_tour_from_a_virtual_point() {
        let request = build_request(&three_places(), 3, Departure::None, TourMode::Open).unwrap();
        assert_eq!(request["vehicles"][0]["start_index"], 3);
        assert_eq!(
            request["matrices"]["car"]["durations"],
            json!([[0, 5, 7, 0], [5, 0, 4, 0], [7, 4, 0, 0], [0, 0, 0, 0]])
        );
    }

    #[test]
    fn refuses_a_matrix_with_a_missing_route() {
        let mut gap = three_places();
        gap[1][2] = None;
        assert!(build_request(&gap, 3, Departure::Index(0), TourMode::Loop).is_err());
    }

    #[test]
    fn reads_the_visit_order() {
        let raw = json!({
            "code": 0,
            "routes": [{ "steps": [{ "type": "start" }, { "type": "job", "id": 2 }, { "type": "job", "id": 0 }, { "type": "job", "id": 1 }, { "type": "end" }] }],
            "unassigned": []
        });
        assert_eq!(parse_solution(&raw, 3).unwrap(), [2, 0, 1]);
    }

    #[test]
    fn rejects_a_solution_with_an_unassigned_place() {
        let raw = json!({
            "code": 0,
            "routes": [{ "steps": [{ "type": "start" }, { "type": "job", "id": 2 }, { "type": "job", "id": 0 }, { "type": "end" }] }],
            "unassigned": [{ "id": 1, "type": "job" }]
        });
        assert!(parse_solution(&raw, 3).is_err());
    }
}
