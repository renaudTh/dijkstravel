//! Géométrie des tracés : distances géodésiques, emprises, chemins mesurés.

use serde::{Deserialize, Serialize};

pub type LonLat = [f64; 2];

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

impl BBox {
    pub fn contains(&self, [lon, lat]: LonLat) -> bool {
        lon >= self.west && lon <= self.east && lat >= self.south && lat <= self.north
    }
}

const EARTH_RADIUS_METERS: f64 = 6_371_008.8;

/// Haversine sur la sphère de rayon moyen.
pub fn distance_meters(a: LonLat, b: LonLat) -> f64 {
    let d_lat = (b[1] - a[1]).to_radians();
    let d_lon = (b[0] - a[0]).to_radians();
    let h =
        (d_lat / 2.0).sin().powi(2) + a[1].to_radians().cos() * b[1].to_radians().cos() * (d_lon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_METERS * h.sqrt().min(1.0).asin()
}

/// `meters[i]` est la distance parcourue en arrivant à `points[i]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeasuredPath {
    pub points: Vec<LonLat>,
    pub meters: Vec<f64>,
}

impl MeasuredPath {
    /// `points` non vide.
    pub fn measure(points: Vec<LonLat>) -> Self {
        let mut total = 0.0;
        let meters = std::iter::once(0.0)
            .chain(points.windows(2).map(|pair| {
                total += distance_meters(pair[0], pair[1]);
                total
            }))
            .collect();
        MeasuredPath { points, meters }
    }

    pub fn total_meters(&self) -> f64 {
        self.meters.last().copied().unwrap_or(0.0)
    }

    /// Indice `i` du segment [i, i + 1] qui contient `meters`, supposé dans ]0, longueur[.
    fn segment_index(&self, meters: f64) -> usize {
        let (mut low, mut high) = (0, self.meters.len() - 1);
        while high - low > 1 {
            let middle = (low + high) >> 1;
            if self.meters[middle] < meters {
                low = middle;
            } else {
                high = middle;
            }
        }
        low
    }

    fn interpolate(&self, index: usize, meters: f64) -> LonLat {
        let a = self.points[index];
        let b = self.points.get(index + 1).copied().unwrap_or(a);
        let start = self.meters[index];
        let length = self.meters.get(index + 1).copied().unwrap_or(start) - start;
        let ratio = if length > 0.0 { (meters - start) / length } else { 0.0 };
        [a[0] + (b[0] - a[0]) * ratio, a[1] + (b[1] - a[1]) * ratio]
    }

    /// Ramené aux extrémités hors de [0, longueur].
    pub fn point_at(&self, meters: f64) -> LonLat {
        if meters <= 0.0 {
            return self.points[0];
        }
        if meters >= self.total_meters() {
            return *self.points.last().unwrap_or(&self.points[0]);
        }
        self.interpolate(self.segment_index(meters), meters)
    }

    /// Tracé entre `from` et `to` mètres, extrémités interpolées ; vide si l'intervalle l'est.
    pub fn path_between(&self, from: f64, to: f64) -> Vec<LonLat> {
        let start = from.max(0.0);
        let end = to.min(self.total_meters());
        if end <= start {
            return Vec::new();
        }
        // `meters` est croissant : les sommets strictement à l'intérieur forment une plage contiguë.
        let first = self.meters.partition_point(|&at| at <= start);
        let last = self.meters.partition_point(|&at| at < end);
        let mut points = Vec::with_capacity(last.saturating_sub(first) + 2);
        points.push(self.point_at(start));
        points.extend_from_slice(&self.points[first..last.max(first)]);
        points.push(self.point_at(end));
        points
    }

    /// Sous-ensemble des sommets espacés d'au moins `min_meters`, distances cumulées d'origine conservées.
    pub fn decimate(&self, min_meters: f64) -> MeasuredPath {
        let mut points = vec![self.points[0]];
        let mut meters = vec![self.meters[0]];
        let mut last_kept = self.meters[0];
        let last_index = self.points.len() - 1;
        for (i, &point) in self.points.iter().enumerate().skip(1) {
            let at = self.meters[i];
            if i != last_index && at - last_kept < min_meters {
                continue;
            }
            points.push(point);
            meters.push(at);
            last_kept = at;
        }
        MeasuredPath { points, meters }
    }
}

pub struct Crossing {
    pub points: Vec<LonLat>,
    pub ferry: bool,
}

/// Tracé parcouru jusqu'à `meters`, découpé en morceaux de route et de traversée en bateau.
pub fn split_crossings(path: &MeasuredPath, ferries: &[[f64; 2]], meters: f64) -> Vec<Crossing> {
    let mut pieces = Vec::new();
    let mut cursor = 0.0;
    for &[from, to] in ferries.iter().chain(std::iter::once(&[f64::INFINITY, f64::INFINITY])) {
        pieces.push(Crossing { points: path.path_between(cursor, from.min(meters)), ferry: false });
        pieces.push(Crossing { points: path.path_between(from, to.min(meters)), ferry: true });
        cursor = to;
    }
    pieces.retain(|piece| piece.points.len() >= 2);
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    // Le long d'un méridien : l'interpolation linéaire et géodésique coïncident.
    fn path() -> MeasuredPath {
        MeasuredPath { points: vec![[2.0, 46.0], [2.0, 47.0], [2.0, 49.0]], meters: vec![0.0, 1000.0, 3000.0] }
    }

    fn rounded([lon, lat]: LonLat) -> LonLat {
        [(lon * 1e9).round() / 1e9, (lat * 1e9).round() / 1e9]
    }

    #[test]
    fn point_at_clamps_and_interpolates() {
        let path = path();
        assert_eq!(rounded(path.point_at(0.0)), [2.0, 46.0]);
        assert_eq!(rounded(path.point_at(3000.0)), [2.0, 49.0]);
        assert_eq!(rounded(path.point_at(2000.0)), [2.0, 48.0]);
        assert_eq!(rounded(path.point_at(-1000.0)), [2.0, 46.0]);
        assert_eq!(rounded(path.point_at(4000.0)), [2.0, 49.0]);
    }

    #[test]
    fn point_at_survives_repeated_points_and_single_point_paths() {
        let repeated =
            MeasuredPath { points: vec![[2.0, 46.0], [2.0, 46.0], [2.0, 47.0]], meters: vec![0.0, 0.0, 1000.0] };
        assert!(!repeated.point_at(0.0).iter().any(|value| value.is_nan()));
        let single = MeasuredPath { points: vec![[2.0, 46.0]], meters: vec![0.0] };
        assert_eq!(single.point_at(500.0), [2.0, 46.0]);
    }

    #[test]
    fn path_between_interpolates_both_ends() {
        let points: Vec<LonLat> = path().path_between(500.0, 2000.0).into_iter().map(rounded).collect();
        assert_eq!(points, vec![[2.0, 46.5], [2.0, 47.0], [2.0, 48.0]]);
        assert!(path().path_between(2000.0, 2000.0).is_empty());
    }

    #[test]
    fn split_crossings_cuts_around_ferries() {
        let path = path();
        let ends = |pieces: Vec<Crossing>| -> Vec<(bool, LonLat, LonLat)> {
            pieces
                .iter()
                .map(|piece| (piece.ferry, rounded(piece.points[0]), rounded(*piece.points.last().unwrap())))
                .collect()
        };
        assert_eq!(split_crossings(&path, &[], 3000.0).len(), 1);
        assert_eq!(
            ends(split_crossings(&path, &[[1000.0, 2000.0]], 3000.0)),
            vec![
                (false, [2.0, 46.0], [2.0, 47.0]),
                (true, [2.0, 47.0], [2.0, 48.0]),
                (false, [2.0, 48.0], [2.0, 49.0])
            ]
        );
        assert_eq!(
            ends(split_crossings(&path, &[[1000.0, 2000.0]], 1500.0)),
            vec![(false, [2.0, 46.0], [2.0, 47.0]), (true, [2.0, 47.0], [2.0, 47.5])]
        );
    }

    #[test]
    fn measure_accumulates_great_circle_distances() {
        let path = MeasuredPath::measure(vec![[0.0, 0.0], [0.0, 1.0], [0.0, 1.0]]);
        assert_eq!(path.meters.len(), 3);
        assert!((path.meters[1] - 111_195.08).abs() < 0.1);
        assert_eq!(path.meters[1], path.meters[2]);
    }
}
