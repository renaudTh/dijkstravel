//! Ce qui accompagne la vidéo sur les réseaux : légende, hashtags et fiche de publication.

use crate::run::Publication;
use crate::tour::Tour;

/// Kilomètres entiers à la française : `25 469` (espace fine insécable entre les milliers).
pub fn format_km(meters: f64) -> String {
    let digits = (meters / 1000.0).round().to_string();
    let mut grouped = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push('\u{202f}');
        }
        grouped.push(digit);
    }
    format!("{grouped} km")
}

impl Publication {
    /// La légende, `{lieux}` et `{km}` remplacés par ceux de la tournée.
    pub fn caption_for(&self, tour: &Tour) -> String {
        self.caption.replace("{lieux}", &tour.stops.len().to_string()).replace("{km}", &format_km(tour.total_meters()))
    }
}

/// La fiche de publication : légende prête à coller, puis ce qu'il reste à préparer. `files` : les
/// lignes sur la vidéo et la couverture, propres à l'endroit où l'app les range.
pub fn sheet(publication: &Publication, tour: &Tour, files: &[String]) -> String {
    let mut lines = vec![
        "=== LÉGENDE (à coller telle quelle) ===".to_owned(),
        String::new(),
        publication.caption_for(tour),
        String::new(),
        publication.hashtags.join(" "),
        String::new(),
        "=== À PRÉPARER ===".to_owned(),
        String::new(),
    ];
    lines.extend(files.iter().cloned());
    lines.extend([
        "Heure : en semaine vers 18 h - 21 h, ou le week-end vers midi (repère général : à ajuster d'après vos statistiques).".to_owned(),
        "Sources : la mention OpenStreetMap / imagerie est déjà dans la vidéo, rien à ajouter dans la légende.".to_owned(),
        String::new(),
    ]);
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands_like_intl_fr() {
        assert_eq!(format_km(25_468_600.0), "25\u{202f}469 km");
        assert_eq!(format_km(999_400.0), "999 km");
        assert_eq!(format_km(1_234_567_000.0), "1\u{202f}234\u{202f}567 km");
    }
}
