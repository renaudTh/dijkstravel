use std::collections::BTreeSet;

use dijkstravel_core::geo::{LonLat, MeasuredPath};
use dijkstravel_core::places::Place;
use dijkstravel_core::run::{Run, Start, TourMode};
use dijkstravel_core::tour::{ExcludedPlace, Stop, Tour};

use crate::Routing;
use crate::osrm::{DistanceMatrix, RoutedPath};
use crate::vroom::{Departure, build_request, parse_solution};

/// Au-delà, OSRM a projeté le lieu sur une route sans rapport (Camopi, sans route, accrochée à 86 km) :
/// le lieu est traité comme inaccessible. Large pour un grand stade, dont le centre est loin de la rue.
pub const MAX_SNAP_METERS: f64 = 2000.0;

#[derive(Debug, Clone, PartialEq)]
pub enum SolveError {
    UnknownStartPlace(String),
    StartUnreachable,
    RouteMismatch { path_meters: f64, routed_meters: f64 },
    NoReachablePlace,
    Osrm(String),
    Vroom(String),
}

impl std::fmt::Display for SolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolveError::UnknownStartPlace(id) => write!(f, "Lieu de départ absent de places.geojson : {id}"),
            SolveError::StartUnreachable => write!(f, "Le point de départ est inaccessible par la route"),
            SolveError::RouteMismatch { path_meters, routed_meters } => {
                write!(f, "Tracé de {path_meters} m pour {routed_meters} m annoncés par OSRM")
            }
            SolveError::NoReachablePlace => write!(f, "Aucun lieu accessible par la route"),
            SolveError::Osrm(reason) => write!(f, "OSRM : {reason}"),
            SolveError::Vroom(reason) => write!(f, "VROOM : {reason}"),
        }
    }
}

impl std::error::Error for SolveError {}

pub fn solve_tour(run: &Run, places: &[Place], routing: &impl Routing) -> Result<Tour, SolveError> {
    let start_place = match &run.start {
        Start::Place(id) => Some(
            places.iter().position(|place| place.id == *id).ok_or_else(|| SolveError::UnknownStartPlace(id.clone()))?,
        ),
        _ => None,
    };
    let start_at = match run.start {
        Start::Coordinates(at) => Some(at),
        _ => None,
    };

    let mut locations: Vec<LonLat> = places.iter().map(|place| place.at).collect();
    locations.extend(start_at);
    let table = routing.table(&locations)?;
    let snapped_away =
        table.snap_meters.iter().enumerate().filter(|&(_, &meters)| meters > MAX_SNAP_METERS).map(|(i, _)| i);
    let unreachable: BTreeSet<usize> = find_unreachable(&table.matrix).into_iter().chain(snapped_away).collect();
    let start_index = if start_at.is_some() { Some(places.len()) } else { start_place };
    if start_index.is_some_and(|index| unreachable.contains(&index)) {
        return Err(SolveError::StartUnreachable);
    }

    let kept: Vec<usize> = (0..locations.len()).filter(|i| !unreachable.contains(i)).collect();
    let reachable: Vec<usize> = (0..places.len()).filter(|i| !unreachable.contains(i)).collect();
    let excluded = unreachable
        .iter()
        .filter_map(|&i| places.get(i))
        .map(|place| ExcludedPlace { id: place.id.clone(), name: place.name.clone() })
        .collect();
    let sub_matrix: DistanceMatrix =
        kept.iter().map(|&from| kept.iter().map(|&to| table.matrix[from][to]).collect()).collect();

    let departure = if start_at.is_some() {
        Departure::Index(reachable.len())
    } else if let Some(place) = start_place {
        Departure::Index(reachable.iter().position(|&i| i == place).expect("départ joignable"))
    } else if run.mode == TourMode::Loop {
        Departure::Index(0)
    } else {
        Departure::None
    };
    let request = build_request(&sub_matrix, reachable.len(), departure, run.mode)?;
    let order = parse_solution(&routing.optimize(&request)?, reachable.len())?;
    let visits: Vec<usize> = order.iter().filter_map(|&job| reachable.get(job).copied()).collect();
    // Le lieu de départ est visité en premier, même si VROOM l'a placé ailleurs à coût nul.
    let origin = match departure {
        Departure::Index(index) if start_at.is_none() => reachable.get(index).copied(),
        _ => None,
    };
    let ordered: Vec<&Place> =
        origin.into_iter().chain(visits.into_iter().filter(|&i| Some(i) != origin)).map(|i| &places[i]).collect();
    if ordered.is_empty() {
        return Err(SolveError::NoReachablePlace);
    }

    let waypoints = route_waypoints(start_at, &ordered, run.mode);
    let route = if waypoints.len() < 2 {
        RoutedPath { points: waypoints, leg_segments: Vec::new(), ferry_segments: Vec::new(), meters: 0.0 }
    } else {
        routing.route(&waypoints)?
    };
    assemble_tour(run, start_at, &ordered, &route, excluded)
}

/// Indices des lieux injoignables depuis la majorité des autres, ou qui ne la rejoignent pas.
pub fn find_unreachable(matrix: &DistanceMatrix) -> Vec<usize> {
    let others = matrix.len().saturating_sub(1);
    let isolated = |nulls: usize| others > 0 && nulls * 2 > others;
    (0..matrix.len())
        .filter(|&i| {
            let row_nulls = (0..matrix.len()).filter(|&j| j != i && matrix[i][j].is_none()).count();
            let column_nulls = (0..matrix.len()).filter(|&j| j != i && matrix[j][i].is_none()).count();
            isolated(row_nulls) || isolated(column_nulls)
        })
        .collect()
}

/// Waypoints à router : départ, lieux dans l'ordre, retour au départ en boucle. `ordered` non vide.
pub fn route_waypoints(start: Option<LonLat>, ordered: &[&Place], mode: TourMode) -> Vec<LonLat> {
    let origin = start.unwrap_or(ordered[0].at);
    let visits = ordered.iter().map(|place| place.at);
    let route: Vec<LonLat> = start.into_iter().chain(visits).collect();
    let mut waypoints = vec![origin];
    waypoints.extend_from_slice(&route[1..]);
    if mode == TourMode::Loop {
        waypoints.push(origin);
    }
    waypoints
}

/// Refuse un tracé dont la longueur s'écarte de plus de 1 % de celle d'OSRM.
pub fn assemble_tour(
    run: &Run,
    start: Option<LonLat>,
    ordered: &[&Place],
    route: &RoutedPath,
    excluded: Vec<ExcludedPlace>,
) -> Result<Tour, SolveError> {
    let path = MeasuredPath::measure(route.points.clone());
    let length = path.total_meters();
    if (length - route.meters).abs() > route.meters * 0.01 {
        return Err(SolveError::RouteMismatch { path_meters: length, routed_meters: route.meters });
    }
    // `arrivals[k]` : indice du point du tracé à la fin de l'étape k.
    let arrivals: Vec<usize> = route
        .leg_segments
        .iter()
        .scan(0, |vertex, &segments| {
            *vertex += segments;
            Some(*vertex)
        })
        .collect();
    let meters_at =
        |vertex: Option<usize>, fallback: f64| path.meters.get(vertex.unwrap_or(0)).copied().unwrap_or(fallback);
    let stops = ordered
        .iter()
        .enumerate()
        .map(|(i, place)| {
            let vertex = match start {
                None if i == 0 => Some(0),
                None => arrivals.get(i - 1).copied(),
                Some(_) => arrivals.get(i).copied(),
            };
            Stop { id: place.id.clone(), name: place.name.clone(), at: place.at, meters: meters_at(vertex, length) }
        })
        .collect();
    let ferries = route
        .ferry_segments
        .iter()
        .map(|&[from, to]| [meters_at(Some(from), 0.0), meters_at(Some(to), length)])
        .collect();
    Ok(Tour {
        run: run.name.clone(),
        mode: run.mode,
        start: start.unwrap_or(ordered[0].at),
        path,
        stops,
        excluded,
        ferries,
        routed_meters: route.meters,
    })
}

#[cfg(test)]
mod tests {
    use dijkstravel_core::geo::distance_meters;
    use dijkstravel_core::run::Run;
    use serde_json::{Value, json};

    use super::*;
    use crate::DistanceTable;

    fn rennes() -> Place {
        Place::new("a", "Rennes", [-1.68, 48.11])
    }
    fn brest() -> Place {
        Place::new("b", "Brest", [-4.49, 48.39])
    }
    fn vannes() -> Place {
        Place::new("c", "Vannes", [-2.76, 47.66])
    }
    fn sein() -> Place {
        Place::new("sein", "Île de Sein", [-4.85, 48.04])
    }
    const PLOERMEL: LonLat = [-2.39, 47.93];

    fn run(start: Value, mode: &str) -> Run {
        let mut raw = json!({
            "name": "solve-test",
            "source": { "kind": "manuel", "description": "test", "date": "2026-10-27", "licence": "CC0" },
            "bbox": { "west": -5.2, "south": 47.2, "east": -1.0, "north": 48.95 },
            "mode": mode
        });
        if !start.is_null() {
            raw["start"] = start;
        }
        Run::parse(&raw).unwrap()
    }

    fn length_of(points: &[LonLat]) -> f64 {
        points.windows(2).map(|pair| distance_meters(pair[0], pair[1])).sum()
    }

    /// Faux OSRM + VROOM : distances haversine, aucune route vers ou depuis `unreachable`, `snapped_away`
    /// projetés à 5 km sur le réseau ; VROOM visite d'abord le job situé au départ du véhicule, puis les
    /// autres dans l'ordre de la requête.
    #[derive(Default)]
    struct FakeRouting {
        unreachable: Vec<LonLat>,
        snapped_away: Vec<LonLat>,
    }

    impl Routing for FakeRouting {
        fn table(&self, locations: &[LonLat]) -> Result<DistanceTable, SolveError> {
            let cut = |point: &LonLat| self.unreachable.contains(point);
            let matrix = locations
                .iter()
                .enumerate()
                .map(|(i, from)| {
                    locations
                        .iter()
                        .enumerate()
                        .map(|(j, to)| match () {
                            () if i == j => Some(0.0),
                            () if cut(from) || cut(to) => None,
                            () => Some(distance_meters(*from, *to)),
                        })
                        .collect()
                })
                .collect();
            let snap_meters =
                locations.iter().map(|point| if self.snapped_away.contains(point) { 5000.0 } else { 3.0 }).collect();
            Ok(DistanceTable { matrix, snap_meters })
        }

        fn route(&self, waypoints: &[LonLat]) -> Result<RoutedPath, SolveError> {
            Ok(RoutedPath {
                points: waypoints.to_vec(),
                leg_segments: vec![1; waypoints.len() - 1],
                ferry_segments: Vec::new(),
                meters: length_of(waypoints),
            })
        }

        fn optimize(&self, request: &Value) -> Result<Value, SolveError> {
            let start = &request["vehicles"][0]["start_index"];
            let jobs = request["jobs"].as_array().unwrap();
            let first = jobs.iter().filter(|job| job["location_index"] == *start);
            let rest = jobs.iter().filter(|job| job["location_index"] != *start);
            let steps: Vec<Value> = first.chain(rest).map(|job| json!({ "type": "job", "id": job["id"] })).collect();
            Ok(json!({ "code": 0, "routes": [{ "steps": steps }], "unassigned": [] }))
        }
    }

    fn loop_route(scale: f64) -> RoutedPath {
        let points = vec![rennes().at, brest().at, vannes().at, rennes().at];
        RoutedPath {
            meters: length_of(&points) * scale,
            points,
            leg_segments: vec![1, 1, 1],
            ferry_segments: Vec::new(),
        }
    }

    fn matrix(rows: &[&[Option<f64>]]) -> DistanceMatrix {
        rows.iter().map(|row| row.to_vec()).collect()
    }

    #[test]
    fn finds_places_that_cannot_be_reached_or_cannot_reach() {
        let (s, n) = (Some, None);
        assert!(find_unreachable(&matrix(&[&[s(0.0), s(5.0)], &[s(5.0), s(0.0)]])).is_empty());
        let unreached = matrix(&[
            &[s(0.0), s(5.0), s(7.0), s(3.0), n],
            &[s(5.0), s(0.0), s(4.0), s(6.0), n],
            &[s(7.0), s(4.0), s(0.0), s(2.0), n],
            &[s(3.0), s(6.0), s(2.0), s(0.0), n],
            &[s(8.0), s(9.0), s(8.0), s(7.0), s(0.0)],
        ]);
        assert_eq!(find_unreachable(&unreached), [4]);
        let stuck = matrix(&[
            &[s(0.0), s(5.0), s(7.0), s(3.0), s(8.0)],
            &[s(5.0), s(0.0), s(4.0), s(6.0), s(9.0)],
            &[s(7.0), s(4.0), s(0.0), s(2.0), s(8.0)],
            &[s(3.0), s(6.0), s(2.0), s(0.0), s(7.0)],
            &[n, n, n, n, s(0.0)],
        ]);
        assert_eq!(find_unreachable(&stuck), [4]);
    }

    #[test]
    fn routes_waypoints_from_the_departure() {
        let (r, b, v) = (rennes(), brest(), vannes());
        assert_eq!(route_waypoints(Some(PLOERMEL), &[&r, &b], TourMode::Open), [PLOERMEL, r.at, b.at]);
        assert_eq!(route_waypoints(Some(PLOERMEL), &[&r, &b], TourMode::Loop).last(), Some(&PLOERMEL));
        assert_eq!(route_waypoints(Some(PLOERMEL), &[&v], TourMode::Loop), [PLOERMEL, v.at, PLOERMEL]);
        assert_eq!(route_waypoints(None, &[&r, &b], TourMode::Loop), [r.at, b.at, r.at]);
    }

    #[test]
    fn assembles_stops_in_order_at_non_decreasing_distances() {
        let (r, b, v) = (rennes(), brest(), vannes());
        let tour = assemble_tour(&run(Value::Null, "loop"), None, &[&r, &b, &v], &loop_route(1.0), Vec::new()).unwrap();
        assert_eq!(tour.stops.iter().map(|stop| stop.id.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);

        let points = vec![PLOERMEL, [-2.0, 48.0], r.at, b.at, [-4.0, 48.0], [-3.5, 47.8], v.at];
        let route =
            RoutedPath { meters: length_of(&points), points, leg_segments: vec![2, 1, 3], ferry_segments: Vec::new() };
        let start = json!({ "lon": PLOERMEL[0], "lat": PLOERMEL[1] });
        let tour = assemble_tour(&run(start, "open"), Some(PLOERMEL), &[&r, &b, &v], &route, Vec::new()).unwrap();
        let meters: Vec<f64> = tour.stops.iter().map(|stop| stop.meters).collect();
        assert!(meters.windows(2).all(|pair| pair[0] <= pair[1]), "{meters:?}");
        assert_eq!(meters[0], tour.path.meters[2]);
    }

    #[test]
    fn measures_ferry_crossings_along_the_path() {
        let (r, b, v) = (rennes(), brest(), vannes());
        let route = RoutedPath { ferry_segments: vec![[1, 2]], ..loop_route(1.0) };
        let tour = assemble_tour(&run(Value::Null, "loop"), None, &[&r, &b, &v], &route, Vec::new()).unwrap();
        assert_eq!(tour.ferries, [[tour.path.meters[1], tour.path.meters[2]]]);
    }

    #[test]
    fn checks_the_path_length_against_osrm_within_one_percent() {
        let (r, b, v) = (rennes(), brest(), vannes());
        let assemble =
            |scale| assemble_tour(&run(Value::Null, "loop"), None, &[&r, &b, &v], &loop_route(scale), Vec::new());
        assert!(matches!(assemble(1.02), Err(SolveError::RouteMismatch { .. })));
        assert!(assemble(1.005).is_ok());
    }

    #[test]
    fn excludes_unreachable_and_far_snapped_places() {
        let places = [rennes(), brest(), vannes(), sein()];
        for routing in [
            FakeRouting { unreachable: vec![sein().at], ..FakeRouting::default() },
            FakeRouting { snapped_away: vec![sein().at], ..FakeRouting::default() },
        ] {
            let tour = solve_tour(&run(Value::Null, "loop"), &places, &routing).unwrap();
            assert_eq!(tour.excluded, [ExcludedPlace { id: "sein".into(), name: "Île de Sein".into() }]);
            let mut ids: Vec<&str> = tour.stops.iter().map(|stop| stop.id.as_str()).collect();
            ids.sort_unstable();
            assert_eq!(ids, ["a", "b", "c"]);
        }
    }

    #[test]
    fn starts_at_the_start_place() {
        let tour =
            solve_tour(&run(json!({ "place": "c" }), "loop"), &[rennes(), brest(), vannes()], &FakeRouting::default())
                .unwrap();
        assert_eq!(tour.path.points[0], vannes().at);
        assert_eq!(tour.stops[0].id, "c");
    }

    #[test]
    fn rejects_an_unknown_start_place() {
        let result = solve_tour(&run(json!({ "place": "nowhere" }), "loop"), &[rennes()], &FakeRouting::default());
        assert_eq!(result.unwrap_err(), SolveError::UnknownStartPlace("nowhere".into()));
    }

    #[test]
    fn starts_a_free_tour_at_a_place_and_closes_a_loop() {
        let places = [rennes(), brest(), vannes()];
        let open = solve_tour(&run(Value::Null, "open"), &places, &FakeRouting::default()).unwrap();
        assert!(places.iter().any(|place| place.at == open.path.points[0]));
        let looped = solve_tour(&run(Value::Null, "loop"), &places, &FakeRouting::default()).unwrap();
        assert_eq!(looped.path.points.last(), Some(&looped.path.points[0]));
    }

    #[test]
    fn rejects_an_unreachable_start() {
        let island = [-5.1, 48.45];
        let routing = FakeRouting { unreachable: vec![island], ..FakeRouting::default() };
        let start = json!({ "lon": island[0], "lat": island[1] });
        let result = solve_tour(&run(start, "loop"), &[rennes(), brest(), vannes()], &routing);
        assert_eq!(result.unwrap_err(), SolveError::StartUnreachable);
    }
}
