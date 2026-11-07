//! Rostering API models and local-time helpers.

use chrono::{DateTime, Local, NaiveDate, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
pub struct ApiSite {
    pub id: String,
    pub name: String,
    pub division_id: Option<String>,
    pub division_name: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub is_active: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiSiteWrite {
    pub name: String,
    pub division_id: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiShift {
    pub id: String,
    pub title: Option<String>,
    pub kind: String,
    pub starts_at: String,
    pub ends_at: String,
    pub location: Option<String>,
    pub status: String,
    pub division_id: Option<String>,
    pub division_name: Option<String>,
    pub service_id: Option<String>,
    pub service_name: Option<String>,
    pub site_id: Option<String>,
    pub site_name: Option<String>,
    pub cost_centre_id: Option<String>,
    pub cost_centre_name: Option<String>,
    pub cost_centre_code: Option<String>,
    pub series_id: Option<String>,
    pub recurrence: Option<String>,
    pub client_count: i64,
    pub worker_count: i64,
    pub clients: Vec<ApiPartyRef>,
    pub workers: Vec<ApiPartyRef>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiShiftList {
    pub items: Vec<ApiShift>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiAssignment {
    pub id: String,
    pub role: String,
    pub contact_id: String,
    pub contact_name: String,
    pub session_id: Option<String>,
    pub status: String,
    pub outcome: Option<String>,
    pub outcome_rating: Option<i16>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiShiftDetail {
    pub shift: ApiShift,
    pub assignments: Vec<ApiAssignment>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiDivision {
    pub id: String,
    pub name: String,
    pub code: Option<String>,
    pub parent_id: Option<String>,
    pub is_active: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiService {
    pub id: String,
    pub name: String,
    pub code: Option<String>,
    pub division_id: Option<String>,
    pub division_name: Option<String>,
    pub is_active: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiCostCentre {
    pub id: String,
    pub code: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub is_active: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiPartyRef {
    pub id: String,
    pub display_name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiContextRef {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiSuitabilityCandidate {
    pub contact_id: String,
    pub display_name: String,
    pub score: f64,
    pub reasons: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiSuitability {
    pub shift_id: String,
    pub site: Option<ApiContextRef>,
    pub service: Option<ApiContextRef>,
    pub division: Option<ApiContextRef>,
    pub clients: Vec<ApiPartyRef>,
    pub assigned_workers: Vec<ApiPartyRef>,
    pub candidates: Vec<ApiSuitabilityCandidate>,
    pub blocked: Vec<ApiSuitabilityCandidate>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiShiftWrite {
    pub title: Option<String>,
    pub kind: Option<String>,
    pub starts_at: String,
    pub ends_at: String,
    pub location: Option<String>,
    pub status: Option<String>,
    pub division_id: Option<String>,
    pub service_id: Option<String>,
    pub site_id: Option<String>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiShiftUpdate {
    pub title: Option<String>,
    pub kind: Option<String>,
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
    pub location: Option<String>,
    pub status: Option<String>,
    pub division_id: Option<String>,
    pub service_id: Option<String>,
    pub site_id: Option<String>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiAssignmentWrite {
    pub contact_id: String,
    pub role: String,
    pub session_id: Option<String>,
    pub status: Option<String>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiBatchShiftWrite {
    pub shifts: Vec<ApiShiftWrite>,
    pub recurrence: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiCopyShiftWrite {
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
    pub include_assignments: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ApiAssignmentUpdate {
    pub status: Option<String>,
    pub outcome: Option<String>,
    pub outcome_rating: Option<i16>,
    pub notes: Option<String>,
}

/// Parse a server UTC timestamp into the local timezone.
pub fn parse_utc(value: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Local))
}

/// Human-readable local date, e.g. `Sat 31 Oct 2026`.
pub fn local_date(value: &str) -> String {
    parse_utc(value)
        .map(|value| value.format("%a %-d %b %Y").to_string())
        .unwrap_or_else(|| value.to_owned())
}

/// Human-readable local time of day, e.g. `6:00 pm`.
pub fn local_time(value: &str) -> String {
    parse_utc(value)
        .map(|value| value.format("%-I:%M %P").to_string())
        .unwrap_or_else(|| value.to_owned())
}

/// Local date key used to group shifts, e.g. `2026-10-31`.
pub fn local_date_key(value: &str) -> String {
    parse_utc(value)
        .map(|value| value.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| value.to_owned())
}

/// Full local range label, e.g. `6:00 pm – 8:00 pm`.
pub fn local_range(start: &str, end: &str) -> String {
    format!("{} – {}", local_time(start), local_time(end))
}

/// The local `(date, time)` inputs for an existing UTC timestamp.
pub fn local_inputs(value: &str) -> (String, String) {
    parse_utc(value)
        .map(|value| {
            (
                value.format("%Y-%m-%d").to_string(),
                value.format("%H:%M").to_string(),
            )
        })
        .unwrap_or_default()
}

/// Turn local date and time inputs into an RFC3339 UTC timestamp.
pub fn local_to_utc(date: &str, time: &str) -> Result<String, String> {
    let date = NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d")
        .map_err(|_| "Date must be YYYY-MM-DD.".to_owned())?;
    let time = NaiveTime::parse_from_str(time.trim(), "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(time.trim(), "%H:%M:%S"))
        .map_err(|_| "Time must be HH:MM (24-hour).".to_owned())?;
    let naive = date.and_time(time);
    let local = Local
        .from_local_datetime(&naive)
        .single()
        .ok_or_else(|| "That local time is ambiguous or invalid.".to_owned())?;
    Ok(local
        .with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Start of a local calendar day as UTC.
pub fn local_day_start(date: &str) -> Result<String, String> {
    local_to_utc(date, "00:00")
}

/// End of a local calendar day as UTC.
pub fn local_day_end(date: &str) -> Result<String, String> {
    local_to_utc(date, "23:59")
}

/// Pretty status label.
pub fn status_label(status: &str) -> String {
    match status {
        "draft" => "Draft".to_owned(),
        "published" => "Published".to_owned(),
        "cancelled" => "Cancelled".to_owned(),
        "completed" => "Completed".to_owned(),
        "assigned" => "Assigned".to_owned(),
        "attended" => "Attended".to_owned(),
        "no_show" => "No show".to_owned(),
        "positive" => "Positive".to_owned(),
        "neutral" => "Neutral".to_owned(),
        "negative" => "Negative".to_owned(),
        other => other.to_owned(),
    }
}

pub fn shift_kind_label(kind: &str) -> String {
    match kind {
        "service_delivery" => "Service delivery".to_owned(),
        "opening_hours" => "Opening hours".to_owned(),
        "respite_program" => "Respite program".to_owned(),
        other => other.to_owned(),
    }
}

/// Monday of the week containing `date`.
pub fn monday_of(date: chrono::NaiveDate) -> chrono::NaiveDate {
    use chrono::Datelike;
    date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64)
}

/// Parse a local date input, or today.
pub fn parse_local_date(value: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
        .unwrap_or_else(|_| Local::now().date_naive())
}

pub fn add_days(date: &str, days: i64) -> String {
    (parse_local_date(date) + chrono::Duration::days(days))
        .format("%Y-%m-%d")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_inputs_round_trip() {
        let utc = local_to_utc("2026-06-15", "09:30").expect("valid local time");
        let (date, time) = local_inputs(&utc);
        assert_eq!(date, "2026-06-15");
        assert_eq!(time, "09:30");
    }

    #[test]
    fn rejects_invalid_dates_and_times() {
        assert!(local_to_utc("15/06/2026", "09:30").is_err());
        assert!(local_to_utc("2026-06-15", "9am").is_err());
        assert!(local_day_start("nope").is_err());
    }

    #[test]
    fn labels_are_human_readable() {
        assert_eq!(status_label("no_show"), "No show");
        assert_eq!(status_label("published"), "Published");
        assert_eq!(shift_kind_label("service_delivery"), "Service delivery");
    }
}
