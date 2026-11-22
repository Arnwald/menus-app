//! Semaines ISO : `2026-W47`, du lundi au dimanche.

use chrono::{Datelike, Duration, IsoWeek, Local, NaiveDate, Weekday};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WeekId {
  pub year: i32,
  pub week: u32,
}

impl WeekId {
  pub fn of(date: NaiveDate) -> Self {
    let iso: IsoWeek = date.iso_week();
    WeekId {
      year: iso.year(),
      week: iso.week(),
    }
  }

  pub fn current() -> Self {
    WeekId::of(Local::now().date_naive())
  }

  /// Semaine à afficher par défaut : la courante, sauf le dimanche après-midi
  /// où l'on prépare déjà la suivante.
  pub fn default_view() -> Self {
    let now = Local::now();
    let week = WeekId::of(now.date_naive());
    if now.weekday() == Weekday::Sun && chrono::Timelike::hour(&now) >= 12 {
      week.add(1)
    } else {
      week
    }
  }

  pub fn monday(self) -> NaiveDate {
    NaiveDate::from_isoywd_opt(self.year, self.week, Weekday::Mon).expect("semaine ISO valide")
  }

  pub fn add(self, weeks: i64) -> Self {
    WeekId::of(self.monday() + Duration::weeks(weeks))
  }

  /// Nombre de semaines de `other` à `self` (positif si `other` est avant).
  pub fn since(self, other: WeekId) -> i64 {
    (self.monday() - other.monday()).num_weeks()
  }

  /// Mois « de la semaine » : celui du jeudi, comme la norme ISO.
  pub fn month(self) -> u32 {
    (self.monday() + Duration::days(3)).month()
  }

  /// « 16–22 nov. 2026 »
  pub fn label(self) -> String {
    const MONTHS: [&str; 12] = [
      "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc.",
    ];
    let start = self.monday();
    let end = start + Duration::days(6);
    let month = |d: NaiveDate| MONTHS[d.month0() as usize];
    if start.month() == end.month() {
      format!("{}–{} {} {}", start.day(), end.day(), month(end), end.year())
    } else if start.year() == end.year() {
      format!("{} {} – {} {} {}", start.day(), month(start), end.day(), month(end), end.year())
    } else {
      format!(
        "{} {} {} – {} {} {}",
        start.day(),
        month(start),
        start.year(),
        end.day(),
        month(end),
        end.year()
      )
    }
  }

  /// Accepte `2026-W47`, `2026-w47`, une date `2026-11-16`, ou `courante`,
  /// `prochaine`, `precedente` (et leurs équivalents anglais).
  pub fn parse_loose(s: &str) -> Result<Self, String> {
    let s = s.trim();
    match crate::slug::fold(s).as_str() {
      "" | "current" | "courante" | "cette" | "now" => return Ok(WeekId::current()),
      "next" | "prochaine" | "suivante" => return Ok(WeekId::current().add(1)),
      "previous" | "prev" | "precedente" | "derniere" => return Ok(WeekId::current().add(-1)),
      _ => {}
    }
    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
      return Ok(WeekId::of(date));
    }
    s.parse()
  }
}

impl fmt::Display for WeekId {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}-W{:02}", self.year, self.week)
  }
}

impl FromStr for WeekId {
  type Err = String;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    let bad = || format!("semaine invalide : « {s} » (attendu AAAA-Wnn)");
    let (year, week) = s.split_once(['W', 'w']).ok_or_else(bad)?;
    let year: i32 = year.trim_end_matches('-').parse().map_err(|_| bad())?;
    let week: u32 = week.parse().map_err(|_| bad())?;
    NaiveDate::from_isoywd_opt(year, week, Weekday::Mon).ok_or_else(bad)?;
    Ok(WeekId { year, week })
  }
}

impl Serialize for WeekId {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(self)
  }
}

impl<'de> Deserialize<'de> for WeekId {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn iso_weeks_round_trip() {
    let date = NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
    let week = WeekId::of(date);
    assert_eq!(week.to_string(), "2026-W47");
    assert_eq!(week.monday(), date);
    assert_eq!("2026-W47".parse::<WeekId>().unwrap(), week);
    assert_eq!(week.label(), "16–22 nov. 2026");
  }

  #[test]
  fn crossing_years() {
    let week: WeekId = "2026-W53".parse().unwrap();
    assert_eq!(week.add(1).to_string(), "2027-W01");
    assert_eq!(week.add(1).since(week), 1);
    assert!("2025-W53".parse::<WeekId>().is_err());
  }
}
