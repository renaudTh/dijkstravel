//! `dijkstravel solve runs/<nom>` → `runs/<nom>/tour.json`. Seul lecteur de `OSRM_URL`, `OSRM_FOOT_URL`,
//! `OSRM_TRAIN_URL`, `OSRM_CORTEGE_URL` et `VROOM_URL`.

use std::path::Path;

use dijkstravel_core::json::to_js_string;
use dijkstravel_core::run::Profile;
use dijkstravel_solve::{Services, solve_tour};

use crate::load::{self, Outcome, fail};

/// Un OSRM par profil : chacun a ses propres données préparées (voir `docker-compose.yml`).
fn osrm_url(profile: Profile) -> String {
    let (variable, fallback) = match profile {
        Profile::Car => ("OSRM_URL", "http://localhost:5000"),
        Profile::Foot => ("OSRM_FOOT_URL", "http://localhost:5001"),
        Profile::Train => ("OSRM_TRAIN_URL", "http://localhost:5002"),
        Profile::Cortege => ("OSRM_CORTEGE_URL", "http://localhost:5003"),
    };
    std::env::var(variable).unwrap_or_else(|_| fallback.to_owned())
}

pub fn main(arg: &Path) -> Outcome {
    let (run, paths) = load::run(arg)?;
    if !paths.places.exists() {
        let source =
            if run.overpass.is_some() { "dijkstravel source overpass" } else { "dijkstravel source csv | atp" };
        return fail(format!("{} absent : le produire d'abord (`{source}`).", paths.places.display()));
    }
    let places = load::places(&run, &paths)?;
    let vroom_url = std::env::var("VROOM_URL").unwrap_or_else(|_| "http://localhost:3000".to_owned());
    let tour = solve_tour(&run, &places, &Services::new(osrm_url(run.profile), vroom_url))?;
    for place in &tour.excluded {
        eprintln!("Inaccessible par la route, exclu : {} ({})", place.id, place.name);
    }
    load::write_file(&paths.tour, to_js_string(&tour))?;
    println!("{} : {} lieux, {:.1} km.", paths.tour.display(), tour.stops.len(), tour.total_meters() / 1000.0);
    Ok(())
}
