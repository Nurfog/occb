//! Deserializador flexible de fechas (grupo A, port origen sin cambios).
//!
//! Acepta RFC 3339, `YYYY-MM-DDTHH:MM:SS` o `YYYY-MM-DD` (medianoche UTC).
//! String vacío o ausente = `None`.

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Deserializer};

pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    match opt {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => {
            if let Ok(dt) = DateTime::parse_from_rfc3339(&s) {
                return Ok(Some(dt.with_timezone(&Utc)));
            }
            if let Ok(naive) = NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S") {
                return Ok(Some(Utc.from_utc_datetime(&naive)));
            }
            if let Ok(date) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
                let naive = date.and_hms_opt(0, 0, 0).unwrap();
                return Ok(Some(Utc.from_utc_datetime(&naive)));
            }
            Err(serde::de::Error::custom(format!(
                "Invalid date format: '{}'. Expected RFC 3339 (e.g., 2026-12-31T00:00:00Z) or YYYY-MM-DD",
                s
            )))
        }
    }
}
