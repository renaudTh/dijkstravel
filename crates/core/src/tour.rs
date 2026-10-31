//! `tour.json` : la tournée calculée, référence de toutes les vidéos du run.

use serde::{Deserialize, Serialize};

use crate::geo::{LonLat, MeasuredPath};
use crate::run::TourMode;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stop {
    pub id: String,
    pub name: String,
    pub at: LonLat,
    /// Distance le long de `Tour::path` à l'arrivée sur ce lieu.
    pub meters: f64,
}

/// Une étape d'une course par étapes, de `from` à `to` mètres le long de `Tour::path`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    /// Affiché dans la carte d'infos tout le long de l'étape.
    pub name: String,
    pub from: f64,
    pub to: f64,
    /// Contre-la-montre : tracé dans `route.timeTrialColor`, cœur en tirets.
    #[serde(rename = "timeTrial", default, skip_serializing_if = "is_false")]
    pub time_trial: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkerKind {
    /// Sprint intermédiaire.
    Sprint,
    /// Ascension classée (col, côte).
    Climb,
    /// Ravitaillement d'une course à pied ; catégorie `eau` : simple point d'eau.
    Supply,
}

/// Un point remarquable d'une étape, visible de son passage à la fin de son étape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub kind: MarkerKind,
    pub name: String,
    /// Catégorie d'une ascension : `HC`, `1` à `4`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub at: LonLat,
    pub meters: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExcludedPlace {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tour {
    pub run: String,
    pub mode: TourMode,
    pub start: LonLat,
    pub path: MeasuredPath,
    pub stops: Vec<Stop>,
    pub excluded: Vec<ExcludedPlace>,
    /// Traversées en bateau (bac, pirogue…) le long de `path`, en mètres `[début, fin]`, dans l'ordre.
    /// Absent des tournées calculées avant les traversées : aucune.
    #[serde(default)]
    pub ferries: Vec<[f64; 2]>,
    /// Transferts entre deux étapes (fin d'une étape → départ de la suivante), en mètres `[début, fin]`,
    /// dans l'ordre : parcourus vite par la caméra, sans tracé, hors de la distance affichée. Absent : aucun.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transfers: Vec<[f64; 2]>,
    /// Étapes d'une course par étapes, dans l'ordre. Absent : aucune.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<Stage>,
    /// Sprints et ascensions des étapes, dans l'ordre du parcours. Absent : aucun.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<Marker>,
    /// Heures de passage d'une activité enregistrée (GPX) : `[mètres le long de path, secondes UTC]`,
    /// croissants. En relief, le soleil et la lune suivent alors l'allure réelle. Absent : aucune.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub timeline: Vec<[f64; 2]>,
    /// Distance annoncée par OSRM, pour contrôler `path`.
    pub routed_meters: f64,
}

impl Tour {
    pub fn parse(text: &str) -> Result<Tour, String> {
        let tour: Tour = serde_json::from_str(text).map_err(|e| format!("tour.json invalide : {e}"))?;
        if tour.path.points.is_empty() || tour.path.points.len() != tour.path.meters.len() {
            return Err("tour.json invalide : path".into());
        }
        Ok(tour)
    }

    pub fn total_meters(&self) -> f64 {
        self.path.total_meters()
    }

    /// L'étape en cours à `meters` : la dernière commencée, celle qu'on vient de finir pendant un transfert.
    pub fn stage_at(&self, meters: f64) -> Option<&Stage> {
        self.stages.iter().take_while(|stage| stage.from <= meters).last()
    }

    /// `meters` tombe à l'intérieur d'un transfert, ses deux bouts exclus.
    pub fn in_transfer(&self, meters: f64) -> bool {
        self.transfers.iter().any(|&[from, to]| from < meters && meters < to)
    }

    /// Distance parcourue hors transferts à `meters` le long de `path`.
    pub fn raced_meters(&self, meters: f64) -> f64 {
        let transferred: f64 = self.transfers.iter().map(|&[from, to]| (meters.min(to) - from).max(0.0)).sum();
        meters - transferred
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::to_js_string;

    #[test]
    fn every_tour_in_the_repository_reads_and_writes_back_identically() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../runs");
        let mut different = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let file = entry.unwrap().path().join("tour.json");
            let Ok(text) = std::fs::read_to_string(&file) else { continue };
            let tour = Tour::parse(&text).unwrap_or_else(|e| panic!("{} : {e}", file.display()));
            let written = to_js_string(&tour);
            if let Some(at) = written.bytes().zip(text.bytes()).position(|(a, b)| a != b) {
                let around = |s: &str| s[at.saturating_sub(40)..(at + 20).min(s.len())].to_owned();
                different.push(format!("{} : {} au lieu de {}", file.display(), around(&written), around(&text)));
            }
        }
        assert!(different.is_empty(), "{different:#?}");
    }

    #[test]
    fn raced_meters_leave_out_the_transfers() {
        let text = r#"{"run":"r","mode":"open","start":[0,0],"path":{"points":[[0,0],[0,1]],"meters":[0,100]},"stops":[],"excluded":[],"transfers":[[20,50]],"routedMeters":100}"#;
        let tour = Tour::parse(text).unwrap();
        assert_eq!([10.0, 35.0, 100.0].map(|meters| tour.raced_meters(meters)), [10.0, 20.0, 70.0]);
        assert_eq!([20.0, 35.0, 50.0].map(|meters| tour.in_transfer(meters)), [false, true, false]);
        assert_eq!(to_js_string(&tour), text.replace("\"transfers\"", "\"ferries\":[],\"transfers\""));
    }

    #[test]
    fn reads_stages_and_markers_and_finds_the_current_stage() {
        let text = r#"{"run":"r","mode":"open","start":[0,0],"path":{"points":[[0,0],[0,1]],"meters":[0,100]},"stops":[],"excluded":[],"ferries":[],"transfers":[[20,50]],"stages":[{"name":"Étape 1","from":0,"to":20},{"name":"Étape 2","from":50,"to":100}],"markers":[{"kind":"climb","name":"Col","category":"HC","at":[0,0.1],"meters":10},{"kind":"sprint","name":"Ville","at":[0,0.6],"meters":60}],"routedMeters":100}"#;
        let tour = Tour::parse(text).unwrap();
        assert_eq!(to_js_string(&tour), text);
        let stage = |meters: f64| tour.stage_at(meters).map(|stage| stage.name.as_str());
        assert_eq!(
            [0.0, 35.0, 50.0, 100.0].map(stage),
            [Some("Étape 1"), Some("Étape 1"), Some("Étape 2"), Some("Étape 2")]
        );
        assert_eq!(tour.markers[0].kind, MarkerKind::Climb);
    }

    #[test]
    fn rejects_a_path_whose_points_and_meters_disagree() {
        let text = r#"{"run":"r","mode":"loop","start":[0,0],"path":{"points":[[0,0]],"meters":[0,1]},"stops":[],"excluded":[],"routedMeters":0}"#;
        assert!(Tour::parse(text).is_err());
    }
}
