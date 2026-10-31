//! Non-régression : valeurs de référence figées, calculées par l'ancienne implémentation TypeScript.
//! `fixtures/real/` (vraies tournées, hors dépôt) est vérifié aussi quand il existe.

use std::path::{Path, PathBuf};

use dijkstravel_render::frame::{Animation, Moment};
use dijkstravel_render::geo::{LonLat, split_crossings};
use dijkstravel_render::model::{CameraTheme, Tour, Video};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    name: String,
    video: Video,
    follow_fraction: f64,
    slowdown: f64,
    tour: Tour,
    frame_count: u32,
    frames: Vec<FrameCase>,
    point_at: Vec<PointCase>,
    crossings: Vec<CrossingCase>,
    decimate: Vec<DecimateCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrameCase {
    frame: u32,
    moment: MomentCase,
    camera: CameraCase,
    hud: HudCase,
}

#[derive(Deserialize)]
struct MomentCase {
    phase: String,
    progress: Option<f64>,
    meters: Option<f64>,
}

#[derive(Deserialize)]
struct CameraCase {
    center: LonLat,
    zoom: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HudCase {
    meters: f64,
    total_meters: f64,
    visited: usize,
    total: usize,
    last_stop: Option<String>,
}

#[derive(Deserialize)]
struct PointCase {
    meters: f64,
    point: LonLat,
}

#[derive(Deserialize)]
struct CrossingCase {
    meters: f64,
    pieces: Vec<PieceCase>,
}

#[derive(Deserialize)]
struct Summary {
    count: usize,
    first: Option<LonLat>,
    last: Option<LonLat>,
    sum: [f64; 2],
}

#[derive(Deserialize)]
struct PieceCase {
    ferry: bool,
    #[serde(flatten)]
    summary: Summary,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DecimateCase {
    min_meters: f64,
    points: Summary,
    meters: f64,
}

/// Écart relatif : les fonctions mathématiques de JS et de Rust peuvent différer d'un ulp.
fn close(actual: f64, expected: f64, what: &str) {
    let scale = expected.abs().max(1.0);
    assert!((actual - expected).abs() <= 1e-9 * scale, "{what} : {actual} au lieu de {expected}");
}

fn close_point(actual: LonLat, expected: LonLat, what: &str) {
    close(actual[0], expected[0], what);
    close(actual[1], expected[1], what);
}

fn check_summary(points: &[LonLat], expected: &Summary, what: &str) {
    assert_eq!(points.len(), expected.count, "{what} : nombre de points");
    if let (Some(&first), Some(first_expected)) = (points.first(), expected.first) {
        close_point(first, first_expected, what);
    }
    if let (Some(&last), Some(last_expected)) = (points.last(), expected.last) {
        close_point(last, last_expected, what);
    }
    let sum = points.iter().fold([0.0, 0.0], |acc, p| [acc[0] + p[0], acc[1] + p[1]]);
    close_point(sum, expected.sum, what);
}

fn check(file: &Path) {
    let fixture: Fixture = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    let name = &fixture.name;
    let tour = std::sync::Arc::new(fixture.tour.clone());
    let animation = Animation::new(
        tour,
        &fixture.video,
        &CameraTheme {
            follow_fraction: fixture.follow_fraction,
            zoom: 1.0,
            stop_slowdown: fixture.slowdown,
            marker_slowdown: 0.0,
            even_stages: 0.0,
            pitch: 0.0,
            heading_window: 1.0,
            terrain_exaggeration: 1.0,
        },
    );
    assert_eq!(animation.timeline.frame_count(), fixture.frame_count, "{name} : nombre d'images");

    for case in &fixture.frames {
        let what = format!("{name} image {}", case.frame);
        let moment = animation.moment_at(case.frame);
        match (moment, case.moment.phase.as_str()) {
            (Moment::Intro { progress }, "intro") | (Moment::Outro { progress }, "outro") => {
                close(progress, case.moment.progress.unwrap(), &what)
            }
            (Moment::Travel { meters }, "travel") => close(meters, case.moment.meters.unwrap(), &what),
            (actual, expected) => panic!("{what} : {actual:?} au lieu de la phase {expected}"),
        }
        let camera = animation.camera_at(moment);
        close_point(camera.center, case.camera.center, &format!("{what} centre"));
        close(camera.zoom, case.camera.zoom, &format!("{what} zoom"));
        let hud = animation.hud_at(animation.meters_of(moment));
        close(hud.meters, case.hud.meters, &format!("{what} hud"));
        close(hud.total_meters, case.hud.total_meters, &format!("{what} hud"));
        assert_eq!(hud.visited, case.hud.visited, "{what} lieux visités");
        assert_eq!(hud.total, case.hud.total, "{what} lieux");
        assert_eq!(hud.last_stop, case.hud.last_stop, "{what} dernier lieu");
    }

    let path = &fixture.tour.path;
    for case in &fixture.point_at {
        close_point(path.point_at(case.meters), case.point, &format!("{name} pointAt({})", case.meters));
    }
    for case in &fixture.crossings {
        let pieces = split_crossings(path, &fixture.tour.ferries, case.meters);
        let what = format!("{name} splitCrossings({})", case.meters);
        assert_eq!(pieces.len(), case.pieces.len(), "{what} : morceaux");
        for (piece, expected) in pieces.iter().zip(&case.pieces) {
            assert_eq!(piece.ferry, expected.ferry, "{what} : traversée");
            check_summary(&piece.points, &expected.summary, &what);
        }
    }
    for case in &fixture.decimate {
        let decimated = path.decimate(case.min_meters);
        let what = format!("{name} decimate({})", case.min_meters);
        check_summary(&decimated.points, &case.points, &what);
        close(decimated.meters.iter().sum(), case.meters, &what);
    }
}

fn fixtures(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| entries.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    files.retain(|file| file.extension().is_some_and(|ext| ext == "json"));
    files.sort();
    files
}

#[test]
fn matches_the_reference_values() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let committed = fixtures(&dir);
    assert!(!committed.is_empty(), "aucune valeur de référence dans tests/fixtures");
    for file in committed.iter().chain(&fixtures(&dir.join("real"))) {
        check(file);
    }
}
