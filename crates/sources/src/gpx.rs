//! Activité enregistrée en GPX (export Strava, montre) : le tracé, l'altitude et l'heure de chaque point,
//! traduits en chemin mesuré, heures de passage et sommets franchis.

use dijkstravel_core::geo::{LonLat, MeasuredPath, distance_meters};

/// Un point du tracé ; l'heure en secondes UTC depuis 1970.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackPoint {
    pub at: LonLat,
    pub elevation: Option<f64>,
    pub time: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    /// `<name>` de la trace.
    pub name: Option<String>,
    /// `<type>` de la trace (`trail_running`, `cycling`…).
    pub sport: Option<String>,
    pub points: Vec<TrackPoint>,
}

/// L'activité ramenée à des points espacés d'au moins quelques mètres, le bruit du GPS à l'arrêt écarté.
#[derive(Debug, Clone, PartialEq)]
pub struct Recorded {
    pub path: MeasuredPath,
    /// Altitude de chaque point de `path`, en mètres (0 sans altitude dans le fichier).
    pub elevations: Vec<f64>,
    /// `[mètres, secondes UTC]` de chaque point de `path`, vide si le fichier n'a pas d'heures (activité
    /// d'un autre exportée par Strava, itinéraire).
    pub timeline: Vec<[f64; 2]>,
}

/// Les points de toutes les traces du fichier, dans l'ordre.
pub fn parse(text: &str) -> Result<Activity, String> {
    let track = text.find("<trk>").map_or(text, |start| &text[start..]);
    let name = element(track, "name").map(unescape);
    let sport = element(track, "type").map(unescape);
    let mut points = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<trkpt") {
        rest = &rest[start + "<trkpt".len()..];
        let tag_end = rest.find('>').ok_or("GPX : balise trkpt non fermée")?;
        let attributes = &rest[..tag_end];
        let body = if attributes.ends_with('/') {
            ""
        } else {
            let end = rest.find("</trkpt>").ok_or("GPX : trkpt non fermé")?;
            &rest[tag_end + 1..end]
        };
        let coordinate = |key: &str| attribute(attributes, key).and_then(|value| value.parse::<f64>().ok());
        let (Some(lat), Some(lon)) = (coordinate("lat"), coordinate("lon")) else {
            return Err(format!("GPX : point {} sans lat ou lon", points.len() + 1));
        };
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
            return Err(format!("GPX : point {} hors du globe", points.len() + 1));
        }
        points.push(TrackPoint {
            at: [lon, lat],
            elevation: element(body, "ele").and_then(|value| value.trim().parse().ok()),
            time: element(body, "time").and_then(parse_time),
        });
    }
    if points.len() < 2 {
        return Err("GPX : moins de deux points de trace (trkpt)".into());
    }
    Ok(Activity { name, sport, points })
}

impl Activity {
    /// Les points espacés d'au moins `min_meters` du dernier gardé, et le dernier ; les heures seulement si
    /// tous les points en ont, croissantes.
    pub fn recorded(&self, min_meters: f64) -> Recorded {
        let mut kept = vec![self.points[0]];
        let mut meters = vec![0.0];
        let mut total = 0.0;
        let mut previous = self.points[0].at;
        let last = self.points.len() - 1;
        for (i, point) in self.points.iter().enumerate().skip(1) {
            let step = distance_meters(previous, point.at);
            if step < min_meters && i != last {
                continue;
            }
            total += step;
            previous = point.at;
            kept.push(*point);
            meters.push(total);
        }
        let elevations = kept.iter().map(|point| point.elevation.unwrap_or(0.0)).collect();
        let timed = kept.iter().all(|point| point.time.is_some())
            && kept.windows(2).all(|pair| pair[0].time <= pair[1].time)
            && kept[0].time < kept[kept.len() - 1].time;
        let timeline = if timed {
            meters.iter().zip(&kept).map(|(&at, point)| [at, point.time.unwrap_or_default()]).collect()
        } else {
            Vec::new()
        };
        Recorded {
            path: MeasuredPath { points: kept.iter().map(|point| point.at).collect(), meters },
            elevations,
            timeline,
        }
    }
}

/// Indices des sommets du profil : chaque point le plus haut entre une montée et une descente d'au moins
/// `drop` mètres, ou monté d'au moins `drop` mètres jusqu'à l'arrivée. Le départ n'en est jamais un.
pub fn summits(elevations: &[f64], drop: f64) -> Vec<usize> {
    let mut found = Vec::new();
    // Indice du plus bas depuis le dernier sommet, ou du plus haut depuis la dernière montée.
    let mut climbing = false;
    let mut extreme = 0;
    for (i, &elevation) in elevations.iter().enumerate() {
        let reference = elevations[extreme];
        if climbing {
            if elevation > reference {
                extreme = i;
            } else if reference - elevation > drop {
                found.push(extreme);
                climbing = false;
                extreme = i;
            }
        } else if elevation < reference {
            extreme = i;
        } else if elevation - reference > drop {
            climbing = true;
            extreme = i;
        }
    }
    if climbing {
        found.push(extreme);
    }
    found
}

/// `2024-08-12T05:34:36Z`, avec fraction de seconde ou décalage `+02:00` en option.
pub fn parse_time(text: &str) -> Option<f64> {
    let text = text.trim();
    let (date, time) = text.split_once('T')?;
    let mut date = date.split('-');
    let year: i64 = date.next()?.parse().ok()?;
    let month: i64 = date.next()?.parse().ok()?;
    let day: i64 = date.next()?.parse().ok()?;
    let (clock, offset) = if let Some(clock) = time.strip_suffix('Z') {
        (clock, 0.0)
    } else if let Some(at) = time.rfind(['+', '-']) {
        let (clock, zone) = time.split_at(at);
        let sign = if zone.starts_with('-') { -1.0 } else { 1.0 };
        let (hours, minutes) = zone[1..].split_once(':').unwrap_or((&zone[1..], "0"));
        (clock, sign * (hours.parse::<f64>().ok()? * 3600.0 + minutes.parse::<f64>().ok()? * 60.0))
    } else {
        (time, 0.0)
    };
    let mut clock = clock.split(':');
    let hour: f64 = clock.next()?.parse().ok()?;
    let minute: f64 = clock.next()?.parse().ok()?;
    let second: f64 = clock.next().map_or(Some(0.0), |second| second.parse().ok())?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour >= 24.0 || minute >= 60.0 || second >= 61.0 {
        return None;
    }
    let days = days_from_civil(year, month, day) as f64;
    Some(days * 86_400.0 + hour * 3600.0 + minute * 60.0 + second - offset)
}

/// Jours depuis le 1er janvier 1970 (calendrier grégorien proleptique).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Le texte du premier `<tag>…</tag>`.
fn element<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&format!("</{tag}>"))?;
    Some(&text[start..start + end])
}

/// La valeur de `key="…"` (ou entre apostrophes) dans les attributs d'une balise.
fn attribute<'a>(attributes: &'a str, key: &str) -> Option<&'a str> {
    let mut rest = attributes;
    loop {
        let at = rest.find(key)?;
        let before_ok = at == 0 || rest[..at].ends_with(char::is_whitespace);
        let after = rest[at + key.len()..].trim_start();
        if before_ok && let Some(value) = after.strip_prefix('=') {
            let value = value.trim_start();
            let quote = value.chars().next()?;
            if quote == '"' || quote == '\'' {
                let value = &value[1..];
                return value.find(quote).map(|end| &value[..end]);
            }
        }
        rest = &rest[at + key.len()..];
    }
}

fn unescape(text: &str) -> String {
    let text = text.trim();
    let text = text.strip_prefix("<![CDATA[").and_then(|inner| inner.strip_suffix("]]>")).unwrap_or(text);
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx creator="StravaGPX" version="1.1">
 <metadata><time>2024-08-12T05:34:36Z</time></metadata>
 <trk>
  <name>La Pra &amp; Aiguebelle</name>
  <type>trail_running</type>
  <trkseg>
   <trkpt lat="45.0000" lon="6.0000"><ele>2000</ele><time>2024-08-12T05:34:36Z</time></trkpt>
   <trkpt lat="45.00001" lon="6.0000"><ele>2000</ele><time>2024-08-12T05:34:40Z</time></trkpt>
   <trkpt lon='6.0000' lat='45.0010'><ele>2010</ele><time>2024-08-12T05:35:36Z</time></trkpt>
   <trkpt lat="45.0020" lon="6.0000"><ele>2005</ele><time>2024-08-12T07:36:36+02:00</time></trkpt>
  </trkseg>
 </trk>
</gpx>"#;

    #[test]
    fn reads_the_points_their_elevation_and_time() {
        let activity = parse(FILE).unwrap();
        assert_eq!(activity.name.as_deref(), Some("La Pra & Aiguebelle"));
        assert_eq!(activity.sport.as_deref(), Some("trail_running"));
        assert_eq!(activity.points.len(), 4);
        assert_eq!(activity.points[2].at, [6.0, 45.001]);
        assert_eq!(activity.points[2].elevation, Some(2010.0));
        // 2024-08-12T05:34:36Z, et la même heure écrite à l'heure d'été de Paris.
        assert_eq!(activity.points[0].time, Some(1_723_440_876.0));
        assert_eq!(activity.points[3].time, Some(1_723_440_876.0 + 120.0));
    }

    #[test]
    fn leaves_out_the_points_closer_than_the_step_but_keeps_their_times() {
        let recorded = parse(FILE).unwrap().recorded(5.0);
        // Le deuxième point, à un mètre du départ, est écarté.
        assert_eq!(recorded.path.points.len(), 3);
        assert_eq!(recorded.elevations, vec![2000.0, 2010.0, 2005.0]);
        assert_eq!(recorded.timeline.len(), 3);
        assert_eq!(recorded.timeline[1][1], 1_723_440_936.0);
        assert!((recorded.timeline[2][0] - 222.4).abs() < 0.5, "{}", recorded.timeline[2][0]);
    }

    #[test]
    fn has_no_timeline_without_times() {
        let untimed = FILE.replace("<time>2024-08-12T05:35:36Z</time>", "");
        assert!(parse(&untimed).unwrap().recorded(5.0).timeline.is_empty());
    }

    #[test]
    fn finds_the_summits_between_two_descents() {
        let profile = [500.0, 900.0, 1900.0, 1700.0, 1950.0, 1200.0, 1600.0, 1100.0, 1500.0];
        assert_eq!(summits(&profile, 300.0), vec![4, 6, 8]);
        assert_eq!(summits(&[500.0, 1000.0, 1050.0], 300.0), vec![2], "arrivée au sommet");
        assert!(summits(&[1000.0, 900.0, 500.0], 300.0).is_empty());
    }

    #[test]
    fn reads_iso_times() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(parse_time("2000-03-01T00:00:00.500Z"), Some(951_868_800.5));
        assert_eq!(parse_time("2024-08-12T07:34:36+02:00"), parse_time("2024-08-12T05:34:36Z"));
        assert_eq!(parse_time("2024-08-12"), None);
    }
}
