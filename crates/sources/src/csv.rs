//! CSV RFC 4180, séparateur virgule, BOM UTF-8 toléré : colonnes `id`, `name`, `lat`, `lon`, et
//! `source` et `image` (adresse d'un blason, d'un logo…) facultatives.

use dijkstravel_core::places::Place;

pub const REQUIRED_COLUMNS: [&str; 4] = ["id", "name", "lat", "lon"];

/// `line` : numéro de la ligne physique où commence l'enregistrement, en-tête = 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvIssue {
    MissingColumn { column: &'static str },
    FieldCount { line: usize, expected: usize, actual: usize },
    UnterminatedQuote { line: usize },
    InvalidNumber { line: usize, column: &'static str, value: String },
}

impl std::fmt::Display for CsvIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CsvIssue::MissingColumn { column } => write!(f, "Colonne manquante dans l'en-tête : {column}."),
            CsvIssue::FieldCount { line, expected, actual } => {
                write!(f, "Ligne {line} : {actual} champs au lieu de {expected} (virgule décimale non protégée ?).")
            }
            CsvIssue::UnterminatedQuote { line } => write!(f, "Ligne {line} : guillemet ouvert jamais refermé."),
            CsvIssue::InvalidNumber { line, column, value } => {
                write!(f, "Ligne {line} : {column} \"{value}\" n'est pas un nombre décimal à point.")
            }
        }
    }
}

struct Record {
    line: usize,
    fields: Vec<String>,
}

fn records(text: &str) -> Result<Vec<Record>, CsvIssue> {
    let mut records = Vec::new();
    let mut fields = Vec::new();
    let mut field = String::new();
    let (mut quoted, mut line, mut record_line) = (false, 1, 1);
    let mut chars = text.chars().peekable();
    let mut push_record = |fields: &mut Vec<String>, record_line: usize| {
        let fields = std::mem::take(fields);
        if fields.iter().any(|value| !value.is_empty()) {
            records.push(Record { line: record_line, fields });
        }
    };
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => {
                    if c == '\n' {
                        line += 1;
                    }
                    field.push(c);
                }
            }
        } else {
            match c {
                '"' => quoted = true,
                ',' => fields.push(std::mem::take(&mut field)),
                '\n' | '\r' => {
                    if c == '\r' && chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    fields.push(std::mem::take(&mut field));
                    push_record(&mut fields, record_line);
                    line += 1;
                    record_line = line;
                }
                _ => field.push(c),
            }
        }
    }
    if quoted {
        return Err(CsvIssue::UnterminatedQuote { line: record_line });
    }
    fields.push(field);
    push_record(&mut fields, record_line);
    Ok(records)
}

/// `-?\d+(\.\d+)?`, espaces autour tolérés : ni virgule décimale ni notation scientifique.
fn parse_number(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    let digits = trimmed.strip_prefix('-').unwrap_or(trimmed);
    let (whole, fraction) = digits.split_once('.').map_or((digits, None), |(w, f)| (w, Some(f)));
    let all_digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    (all_digits(whole) && fraction.is_none_or(all_digits)).then(|| trimmed.parse().ok()).flatten()
}

pub fn parse_places_csv(text: &str) -> Result<Vec<Place>, Vec<CsvIssue>> {
    let records = records(text.strip_prefix('\u{feff}').unwrap_or(text)).map_err(|issue| vec![issue])?;
    let mut records = records.into_iter();
    let columns: Vec<String> = records
        .next()
        .map(|header| header.fields.iter().map(|name| name.trim().to_owned()).collect())
        .unwrap_or_default();
    let missing: Vec<CsvIssue> = REQUIRED_COLUMNS
        .into_iter()
        .filter(|column| !columns.iter().any(|name| name == column))
        .map(|column| CsvIssue::MissingColumn { column })
        .collect();
    if !missing.is_empty() {
        return Err(missing);
    }
    let index = |column: &str| columns.iter().position(|name| name == column);
    let mut issues = Vec::new();
    let mut places = Vec::new();
    for Record { line, fields } in records {
        if fields.len() != columns.len() {
            issues.push(CsvIssue::FieldCount { line, expected: columns.len(), actual: fields.len() });
            continue;
        }
        let at = |column: &str| index(column).map_or("", |i| fields[i].as_str());
        let mut number = |column: &'static str| {
            let parsed = parse_number(at(column));
            if parsed.is_none() {
                issues.push(CsvIssue::InvalidNumber { line, column, value: at(column).to_owned() });
            }
            parsed
        };
        let (lat, lon) = (number("lat"), number("lon"));
        let (Some(lat), Some(lon)) = (lat, lon) else { continue };
        let mut place = Place::new(at("id"), at("name"), [lon, lat]);
        place.source = Some(at("source").to_owned()).filter(|source| !source.is_empty());
        place.image = Some(at("image").to_owned()).filter(|image| !image.is_empty());
        places.push(place);
    }
    if issues.is_empty() { Ok(places) } else { Err(issues) }
}

#[cfg(test)]
mod tests {
    use dijkstravel_core::geo::BBox;
    use dijkstravel_core::places::{parse_places, to_places_file};

    use super::*;

    const BRITTANY: BBox = BBox { west: -5.2, south: 47.2, east: -1.0, north: 48.95 };

    fn lines_of(result: Result<Vec<Place>, Vec<CsvIssue>>) -> Vec<usize> {
        result
            .unwrap_err()
            .iter()
            .map(|issue| match issue {
                CsvIssue::FieldCount { line, .. }
                | CsvIssue::InvalidNumber { line, .. }
                | CsvIssue::UnterminatedQuote { line } => *line,
                CsvIssue::MissingColumn { .. } => 0,
            })
            .collect()
    }

    #[test]
    fn converts_a_conforming_csv_into_valid_places() {
        let places =
            parse_places_csv("\u{feff}id,name,lat,lon,source,image\r\na,Rennes,48.11,-1.68,mairie.fr,https://x.org/r.svg\r\nb,Brest, 48.39 ,-4.49,,\r\n")
                .unwrap();
        assert_eq!(places[0].image.as_deref(), Some("https://x.org/r.svg"));
        assert_eq!(places[1].image, None);
        assert_eq!(places[0].at, [-1.68, 48.11]);
        assert_eq!(places[0].source.as_deref(), Some("mairie.fr"));
        assert_eq!(places[1].source, None);
        assert!(parse_places(&to_places_file(&places).to_string(), &BRITTANY).is_ok());
    }

    #[test]
    fn unquotes_commas_and_escaped_quotes() {
        let places =
            parse_places_csv("id,name,lat,lon\nfoix,\"Château de Foix, \"\"le comtal\"\"\",42.96,1.6\n").unwrap();
        assert_eq!(places[0].name, "Château de Foix, \"le comtal\"");
    }

    #[test]
    fn rejects_decimal_commas_citing_the_line() {
        assert_eq!(lines_of(parse_places_csv("id,name,lat,lon\na,Rennes,48.11,-1.68\nb,Brest,\"48,39\",-4.49\n")), [3]);
        assert_eq!(lines_of(parse_places_csv("id,name,lat,lon\na,Rennes,48.11,-1.68\nb,Brest,48,39,-4.49\n")), [3]);
    }

    #[test]
    fn counts_lines_inside_quoted_fields() {
        assert_eq!(
            lines_of(parse_places_csv("id,name,lat,lon\na,\"deux\nlignes\",48.11,-1.68\nb,Brest,x,-4.49\n")),
            [4]
        );
        assert_eq!(lines_of(parse_places_csv("id,name,lat,lon\na,\"Rennes,48.11,-1.68\n")), [2]);
    }

    #[test]
    fn names_a_missing_required_column() {
        assert_eq!(
            parse_places_csv("id,name,lon\na,Rennes,-1.68\n"),
            Err(vec![CsvIssue::MissingColumn { column: "lat" }])
        );
    }

    #[test]
    fn reads_only_plain_decimal_numbers() {
        assert_eq!(parse_number(" -1.68 "), Some(-1.68));
        assert_eq!(parse_number("48"), Some(48.0));
        for invalid in ["", "1e3", "48,1", ".5", "1.", "--1"] {
            assert_eq!(parse_number(invalid), None, "{invalid}");
        }
    }
}
