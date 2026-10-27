//! `dijkstravel publication runs/<nom>` → `data/out/<nom>/publication.txt` (légende prête à coller) et
//! `data/out/<nom>/cover.jpg` (dernière image, tracé complet dézoomé, sans la carte d'infos : place
//! libre pour un titre).

use std::path::Path;

use dijkstravel_core::publication::sheet;

use crate::load::{self, Outcome, fail};
use crate::render::{console, scene, write_image};

pub fn main(arg: &Path) -> Outcome {
    let (scene, paths) = scene(arg)?;
    let Some(publication) = &scene.run.publication else {
        return fail(format!("{} : aucune section publication (caption, hashtags).", paths.file.display()));
    };
    let renderer = scene.renderer(load::theme(&scene.run)?, None)?;
    let cover = paths.out.join("cover.jpg");
    write_image(&scene.cover(&renderer, &console(&scene))?, &cover)?;

    let video = &paths.video;
    let files = [
        if video.exists() {
            format!("Vidéo : {}", video.display())
        } else {
            format!("Vidéo : pas encore rendue → dijkstravel render {}", arg.display())
        },
        format!(
            "Couverture : {} (tracé complet dézoomé, sans la carte de titre, à choisir comme miniature)",
            cover.display()
        ),
    ];
    let text = sheet(publication, &scene.tour, &files);
    load::write_file(&paths.out.join("publication.txt"), &text)?;
    println!("{text}");
    Ok(())
}
