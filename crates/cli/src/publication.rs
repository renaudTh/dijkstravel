//! `dijkstravel publication runs/<nom>` → `data/out/<nom>/publication.txt` (légende prête à coller) et
//! `data/out/<nom>/cover.jpg` (dernière image, tracé complet dézoomé, sans la carte d'infos : place
//! libre pour un titre).

use std::path::Path;

use crate::load::{self, Outcome, fail};
use crate::render::{Scene, write_frame};

/// Kilomètres entiers à la française : `25 469` (espace fine insécable entre les milliers).
fn format_km(meters: f64) -> String {
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

pub fn main(arg: &Path) -> Outcome {
    let scene = Scene::load(arg)?;
    let Some(publication) = &scene.run.publication else {
        return fail(format!("{} : aucune section publication (caption, hashtags).", scene.paths.file.display()));
    };
    let caption = publication
        .caption
        .replace("{lieux}", &scene.tour.stops.len().to_string())
        .replace("{km}", &format_km(scene.tour.total_meters()));

    let renderer = scene.renderer()?;
    let last = renderer.frame_count().saturating_sub(1);
    scene.prefetch(&renderer, last..last + 1, true)?;
    let cover = scene.paths.out.join("cover.jpg");
    // Toute dernière image : fin de l'outro, tracé complet dézoomé, sans la carte d'infos.
    write_frame(&renderer, last, &cover, false)?;

    let video = &scene.paths.video;
    let text = [
        "=== LÉGENDE (à coller telle quelle) ===".to_owned(),
        String::new(),
        caption,
        String::new(),
        publication.hashtags.join(" "),
        String::new(),
        "=== À PRÉPARER ===".to_owned(),
        String::new(),
        if video.exists() {
            format!("Vidéo : {}", video.display())
        } else {
            format!("Vidéo : pas encore rendue → dijkstravel render {}", arg.display())
        },
        format!("Couverture : {} (tracé complet dézoomé, sans la carte de titre, à choisir comme miniature)", cover.display()),
        "Heure : en semaine vers 18 h - 21 h, ou le week-end vers midi (repère général : à ajuster d'après vos statistiques).".to_owned(),
        "Sources : la mention OpenStreetMap / imagerie est déjà dans la vidéo, rien à ajouter dans la légende.".to_owned(),
        String::new(),
    ]
    .join("\n");
    load::write_file(&scene.paths.out.join("publication.txt"), &text)?;
    println!("{text}");
    Ok(())
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
