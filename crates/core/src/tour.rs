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
    fn rejects_a_path_whose_points_and_meters_disagree() {
        let text = r#"{"run":"r","mode":"loop","start":[0,0],"path":{"points":[[0,0]],"meters":[0,1]},"stops":[],"excluded":[],"routedMeters":0}"#;
        assert!(Tour::parse(text).is_err());
    }
}
