//! Portage de `src/geo.ts` : mêmes règles, mêmes résultats (vérifiés par `tests/parity.rs`).

use serde::Deserialize;

pub type LonLat = [f64; 2];

/// `meters[i]` est la distance parcourue en arrivant à `points[i]`.
#[derive(Debug, Clone, Deserialize)]
pub struct MeasuredPath {
    pub points: Vec<LonLat>,
    pub meters: Vec<f64>,
}

impl MeasuredPath {
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
