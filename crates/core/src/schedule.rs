//! Horaires d'un run (`schedule`) : départ et arrivée en heure locale du lieu, `2026-07-24T05:30`. Le
//! fuseau se déduit du lieu ; c'est l'app qui le cherche et en tire l'instant.

use serde_json::Value;

use crate::json::field;

/// Date et heure sans fuseau, à la minute ou à la seconde.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalDateTime {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl LocalDateTime {
    /// `AAAA-MM-JJTHH:MM`, ou `AAAA-MM-JJ HH:MM`, avec `:SS` en option.
    pub fn parse(text: &str) -> Option<LocalDateTime> {
        let text = text.trim();
        let (date, time) = text.split_once(['T', ' '])?;
        let mut date = date.split('-');
        let year: i32 = number(date.next()?, 4)?.try_into().ok()?;
        let month = number(date.next()?, 2)? as u8;
        let day = number(date.next()?, 2)? as u8;
        let mut time = time.split(':');
        let hour = number(time.next()?, 2)? as u8;
        let minute = number(time.next()?, 2)? as u8;
        let second = time.next().map_or(Some(0), |second| number(second, 2))? as u8;
        if date.next().is_some() || time.next().is_some() {
            return None;
        }
        let valid = (1..=12).contains(&month)
            && (1..=days_in_month(year, month)).contains(&day)
            && hour < 24
            && minute < 60
            && second < 60;
        valid.then_some(LocalDateTime { year, month, day, hour, minute, second })
    }

    /// `20260724-0530`, pour un nom de fichier.
    pub fn compact(&self) -> String {
        format!("{:04}{:02}{:02}-{:02}{:02}", self.year, self.month, self.day, self.hour, self.minute)
    }
}

impl std::fmt::Display for LocalDateTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}T{:02}:{:02}", self.year, self.month, self.day, self.hour, self.minute)?;
        if self.second > 0 {
            write!(f, ":{:02}", self.second)?;
        }
        Ok(())
    }
}

/// Départ et arrivée, en heure locale du départ et de l'arrivée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub start: LocalDateTime,
    pub finish: LocalDateTime,
}

impl Schedule {
    /// `{ "start": "2026-07-24T05:30", "finish": "2026-07-25T09:10" }`, l'arrivée après le départ (à l'heure
    /// locale : un parcours qui change de fuseau n'est pas vérifié ici).
    pub fn parse(raw: &Value) -> Option<Schedule> {
        let at = |key: &str| field(raw, key).and_then(Value::as_str).and_then(LocalDateTime::parse);
        let schedule = Schedule { start: at("start")?, finish: at("finish")? };
        (schedule.finish > schedule.start).then_some(schedule)
    }
}

/// Entier positif de `digits` chiffres exactement.
fn number(text: &str, digits: usize) -> Option<u32> {
    if text.len() != digits || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn reads_local_date_times_to_the_minute_or_second() {
        let at = LocalDateTime::parse("2026-07-24T05:30").unwrap();
        assert_eq!((at.year, at.month, at.day, at.hour, at.minute, at.second), (2026, 7, 24, 5, 30, 0));
        assert_eq!(LocalDateTime::parse("2026-07-24 05:30:15").unwrap().second, 15);
        assert_eq!(at.to_string(), "2026-07-24T05:30");
        assert_eq!(at.compact(), "20260724-0530");
        for wrong in ["2026-07-24", "2026-7-24T05:30", "2026-02-29T10:00", "2026-07-24T24:00", "2026-07-24T05:30Z"] {
            assert_eq!(LocalDateTime::parse(wrong), None, "{wrong}");
        }
        assert!(LocalDateTime::parse("2028-02-29T10:00").is_some(), "année bissextile");
    }

    #[test]
    fn the_finish_comes_after_the_start() {
        let schedule = |start: &str, finish: &str| Schedule::parse(&json!({ "start": start, "finish": finish }));
        assert!(schedule("2026-07-24T21:00", "2026-07-25T06:15").is_some());
        assert!(schedule("2026-07-24T21:00", "2026-07-24T20:00").is_none());
        assert!(Schedule::parse(&json!({ "start": "2026-07-24T21:00" })).is_none());
    }
}
