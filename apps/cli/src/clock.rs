//! Horaires du run (`schedule`) en instants : chaque heure locale se lit dans le fuseau du lieu où elle
//! se passe (le départ, l'arrivée), cherché d'après ses coordonnées, heure d'été comprise.

use dijkstravel_core::geo::LonLat;
use dijkstravel_core::schedule::{LocalDateTime, Schedule};
use dijkstravel_render::model::Clock;

/// Les instants du départ, à `start`, et de l'arrivée, à `finish`.
pub fn resolve(schedule: Schedule, start: LonLat, finish: LonLat) -> Result<Clock, String> {
    let finder = tzf_rs::DefaultFinder::new();
    let instant = |at: LocalDateTime, [lon, lat]: LonLat| -> Result<f64, String> {
        let zone = finder.get_tz_name(lon, lat);
        let tz = jiff::tz::TimeZone::get(zone).map_err(|error| format!("fuseau {zone} : {error}"))?;
        let civil = jiff::civil::DateTime::new(
            at.year as i16,
            at.month as i8,
            at.day as i8,
            at.hour as i8,
            at.minute as i8,
            at.second as i8,
            0,
        )
        .map_err(|error| format!("{at} : {error}"))?;
        let zoned = civil.to_zoned(tz).map_err(|error| format!("{at} dans {zone} : {error}"))?;
        Ok(zoned.timestamp().as_second() as f64)
    };
    let clock = Clock { start: instant(schedule.start, start)?, finish: instant(schedule.finish, finish)? };
    if clock.finish <= clock.start {
        return Err(format!(
            "arrivée ({}) avant le départ ({}) une fois les fuseaux pris en compte",
            schedule.finish, schedule.start
        ));
    }
    Ok(clock)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> LocalDateTime {
        LocalDateTime::parse(text).unwrap()
    }

    #[test]
    fn reads_local_hours_in_the_time_zone_of_the_place() {
        let paris = [2.35, 48.86];
        // Heure d'été en juillet (UTC + 2), d'hiver en janvier (UTC + 1).
        let summer = resolve(Schedule { start: at("2026-07-24T05:30"), finish: at("2026-07-24T07:30") }, paris, paris);
        assert_eq!(summer.unwrap().start, 1_784_863_800.0);
        let winter = resolve(Schedule { start: at("2026-01-24T05:30"), finish: at("2026-01-24T07:30") }, paris, paris);
        assert_eq!(winter.unwrap().start, 1_769_229_000.0);
        // Départ à Lisbonne (UTC + 1 l'été), arrivée à Paris (UTC + 2) : une heure de trajet.
        let lisbon = [-9.14, 38.72];
        let trip = resolve(Schedule { start: at("2026-07-24T10:00"), finish: at("2026-07-24T12:00") }, lisbon, paris);
        let trip = trip.unwrap();
        assert_eq!(trip.finish - trip.start, 3_600.0);
    }
}
