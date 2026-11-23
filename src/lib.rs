use chrono::{Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Tsify)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct CycleRecord {
    pub id: u64,
    pub date: String, // YYYY-MM-DD
    pub flow: String,
    pub notes: String,
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct RecordViewItem {
    pub id: u64,
    pub date: String,
    pub flow: String,
    pub notes: String,
    pub interval_days: Option<i64>,
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct CalendarDay {
    pub day: Option<u32>, // None = leading empty padding cell before the 1st of the month
    pub date_str: String, // "YYYY-MM-DD" or ""
    pub status: String,   // "none" | "logged" | "predicted" | "pms" | "ovulation"
    pub is_today: bool,
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct DashboardState {
    pub has_records: bool,
    pub avg_cycle: i64,
    pub cycle_day: Option<i64>,
    pub phase_name: String,
    pub care_tip: String,
    pub days_until_next: Option<i64>,
    pub next_period_date: Option<String>,
    pub pms_start_date: Option<String>,
    pub ovulation_date: Option<String>,
    pub calendar_year: i32,
    pub calendar_month: u32,
    pub calendar_days: Vec<CalendarDay>,
    pub history: Vec<RecordViewItem>,
}

#[wasm_bindgen]
pub struct CycleStore {
    records: Vec<CycleRecord>,
}

#[wasm_bindgen]
impl CycleStore {
    #[wasm_bindgen(constructor)]
    pub fn new(raw_json: Option<String>) -> Self {
        let mut records = raw_json
            .and_then(|s| serde_json::from_str::<Vec<CycleRecord>>(&s).ok())
            .unwrap_or_default();
        records.sort_by(|a, b| b.date.cmp(&a.date));
        Self { records }
    }

    pub fn add_record(
        &mut self,
        id: u64,
        date_str: &str,
        flow: &str,
        notes: &str,
    ) -> Result<String, JsValue> {
        NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .map_err(|_| JsValue::from_str("Invalid date format (expected YYYY-MM-DD)"))?;

        if self.records.iter().any(|r| r.date == date_str) {
            return Err(JsValue::from_str("A record already exists for this date"));
        }

        self.records.push(CycleRecord {
            id,
            date: date_str.to_string(),
            flow: flow.to_string(),
            notes: notes.to_string(),
        });

        self.records.sort_by(|a, b| b.date.cmp(&a.date));
        self.export_snapshot()
    }

    pub fn delete_record(&mut self, id: u64) -> String {
        self.records.retain(|r| r.id != id);
        self.export_snapshot().unwrap_or_else(|_| "[]".to_string())
    }

    pub fn export_snapshot(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.records).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn get_dashboard_state(
        &self,
        today_str: &str,
        view_year: i32,
        view_month: u32,
    ) -> Result<JsValue, JsValue> {
        let today = NaiveDate::parse_from_str(today_str, "%Y-%m-%d")
            .map_err(|_| JsValue::from_str("Invalid today date format"))?;

        if self.records.is_empty() {
            let empty_grid = build_calendar_grid(view_year, view_month, today, &[], None, 28);
            let empty = DashboardState {
                has_records: false,
                avg_cycle: 28,
                cycle_day: None,
                phase_name: "No Records Yet".to_string(),
                care_tip: "Log at least one period start date to calculate cycle phases and reminders.".to_string(),
                days_until_next: None,
                next_period_date: None,
                pms_start_date: None,
                ovulation_date: None,
                calendar_year: view_year,
                calendar_month: view_month,
                calendar_days: empty_grid,
                history: vec![],
            };
            return serde_wasm_bindgen::to_value(&empty)
                .map_err(|e| JsValue::from_str(&e.to_string()));
        }

        let (avg_cycle, parsed_dates, history_view) = self.compute_stats();
        let last_start = parsed_dates[0];
        let cycle_day = (today - last_start).num_days() + 1;
        let next_period = last_start + Duration::days(avg_cycle);
        let pms_start = next_period - Duration::days(5);
        let ovulation = next_period - Duration::days(14);
        let days_until_next = (next_period - today).num_days();

        let (phase_name, care_tip) = evaluate_phase(cycle_day, avg_cycle, avg_cycle - 14);
        let grid = build_calendar_grid(
            view_year,
            view_month,
            today,
            &parsed_dates,
            Some(next_period),
            avg_cycle,
        );

        let state = DashboardState {
            has_records: true,
            avg_cycle,
            cycle_day: Some(cycle_day),
            phase_name,
            care_tip,
            days_until_next: Some(days_until_next),
            next_period_date: Some(next_period.format("%Y-%m-%d").to_string()),
            pms_start_date: Some(pms_start.format("%Y-%m-%d").to_string()),
            ovulation_date: Some(ovulation.format("%Y-%m-%d").to_string()),
            calendar_year: view_year,
            calendar_month: view_month,
            calendar_days: grid,
            history: history_view,
        };

        serde_wasm_bindgen::to_value(&state).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn generate_ics(&self) -> Result<String, JsValue> {
        if self.records.is_empty() {
            return Err(JsValue::from_str("No records available to export"));
        }

        let (avg_cycle, parsed_dates, _) = self.compute_stats();
        let last_start = parsed_dates[0];
        let next_period = last_start + Duration::days(avg_cycle);
        let pms_start = next_period - Duration::days(5);
        let now_stamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();

        let snapshot_json = serde_json::to_string(&self.records)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let ics = format!(
            "BEGIN:VCALENDAR\r\n\
             VERSION:2.0\r\n\
             PRODID:-//MoonPhase//CycleEngine//EN\r\n\
             CALSCALE:GREGORIAN\r\n\
             METHOD:PUBLISH\r\n\
             X-CYCLE-BACKUP:{}\r\n\
             BEGIN:VEVENT\r\n\
             UID:pms-{}@moonphase.local\r\n\
             DTSTAMP:{}\r\n\
             DTSTART;VALUE=DATE:{}\r\n\
             DTEND;VALUE=DATE:{}\r\n\
             SUMMARY:PMS Window Starts\r\n\
             DESCRIPTION:Late luteal phase (~5 days prior). Keep plans low-stress and comfort snacks handy.\r\n\
             BEGIN:VALARM\r\n\
             ACTION:DISPLAY\r\n\
             DESCRIPTION:PMS Window Reminder\r\n\
             TRIGGER:PT9H\r\n\
             END:VALARM\r\n\
             END:VEVENT\r\n\
             BEGIN:VEVENT\r\n\
             UID:period-{}@moonphase.local\r\n\
             DTSTAMP:{}\r\n\
             DTSTART;VALUE=DATE:{}\r\n\
             DTEND;VALUE=DATE:{}\r\n\
             SUMMARY:Expected Cycle Start (Day 1)\r\n\
             DESCRIPTION:Based on {}-day avg cycle. Keep warm drinks and pain relief ready.\r\n\
             BEGIN:VALARM\r\n\
             ACTION:DISPLAY\r\n\
             DESCRIPTION:Expected Cycle Starts Tomorrow\r\n\
             TRIGGER:-PT15H\r\n\
             END:VALARM\r\n\
             BEGIN:VALARM\r\n\
             ACTION:DISPLAY\r\n\
             DESCRIPTION:Expected Cycle Day 1\r\n\
             TRIGGER:PT9H\r\n\
             END:VALARM\r\n\
             END:VEVENT\r\n\
             END:VCALENDAR",
            snapshot_json,
            pms_start.format("%Y%m%d"),
            now_stamp,
            pms_start.format("%Y%m%d"),
            (pms_start + Duration::days(1)).format("%Y%m%d"),
            next_period.format("%Y%m%d"),
            now_stamp,
            next_period.format("%Y%m%d"),
            (next_period + Duration::days(1)).format("%Y%m%d"),
            avg_cycle
        );

        Ok(ics)
    }

    pub fn import_from_ics(&mut self, ics_content: &str) -> Result<String, JsValue> {
        let unfolded = ics_content.replace("\r\n ", "").replace("\n ", "");
        let backup_line = unfolded
            .lines()
            .find(|line| line.starts_with("X-CYCLE-BACKUP:"))
            .ok_or_else(|| JsValue::from_str("No embedded cycle backup found in this .ics file"))?;

        let raw_json = backup_line.trim_start_matches("X-CYCLE-BACKUP:");
        let mut imported: Vec<CycleRecord> = serde_json::from_str(raw_json)
            .map_err(|e| JsValue::from_str(&format!("Corrupted backup payload: {}", e)))?;

        imported.sort_by(|a, b| b.date.cmp(&a.date));
        self.records = imported;
        self.export_snapshot()
    }
}

impl CycleStore {
    fn compute_stats(&self) -> (i64, Vec<NaiveDate>, Vec<RecordViewItem>) {
        let parsed_dates: Vec<NaiveDate> = self
            .records
            .iter()
            .filter_map(|r| NaiveDate::parse_from_str(&r.date, "%Y-%m-%d").ok())
            .collect();

        let mut total_diff = 0i64;
        let mut valid_intervals = 0i64;
        let mut history_view = Vec::with_capacity(self.records.len());

        for (i, rec) in self.records.iter().enumerate() {
            let interval = if i + 1 < parsed_dates.len() {
                let diff = (parsed_dates[i] - parsed_dates[i + 1]).num_days();
                if (18..=60).contains(&diff) {
                    total_diff += diff;
                    valid_intervals += 1;
                }
                Some(diff)
            } else {
                None
            };

            history_view.push(RecordViewItem {
                id: rec.id,
                date: rec.date.clone(),
                flow: rec.flow.clone(),
                notes: rec.notes.clone(),
                interval_days: interval,
            });
        }

        let avg_cycle = if valid_intervals > 0 {
            (total_diff as f64 / valid_intervals as f64).round() as i64
        } else {
            28
        };

        (avg_cycle, parsed_dates, history_view)
    }
}

fn build_calendar_grid(
    year: i32,
    month: u32,
    today: NaiveDate,
    logged_starts: &[NaiveDate],
    next_period_opt: Option<NaiveDate>,
    avg_cycle: i64,
) -> Vec<CalendarDay> {
    let first_of_month = match NaiveDate::from_ymd_opt(year, month, 1) {
        Some(d) => d,
        None => return vec![],
    };

    let next_month_first = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1).unwrap()
    };
    let days_in_month = (next_month_first - first_of_month).num_days() as u32;

    let leading_blanks = first_of_month.weekday().num_days_from_sunday();
    let mut cells = Vec::with_capacity((leading_blanks + days_in_month) as usize);

    for _ in 0..leading_blanks {
        cells.push(CalendarDay {
            day: None,
            date_str: String::new(),
            status: "none".to_string(),
            is_today: false,
        });
    }

    let mut predicted_starts = Vec::new();
    if let Some(first_pred) = next_period_opt {
        for k in 0..3 {
            predicted_starts.push(first_pred + Duration::days(avg_cycle * k));
        }
    }

    for d in 1..=days_in_month {
        let cell_date = NaiveDate::from_ymd_opt(year, month, d).unwrap();
        let mut status = "none";

        let is_logged = logged_starts.iter().any(|&start| {
            let offset = (cell_date - start).num_days();
            (0..5).contains(&offset)
        });

        if is_logged {
            status = "logged";
        } else {
            for &pred_start in &predicted_starts {
                let offset = (cell_date - pred_start).num_days();
                if (0..5).contains(&offset) {
                    status = "predicted";
                    break;
                } else if (-5..0).contains(&offset) {
                    status = "pms";
                    break;
                } else if (-15..=-13).contains(&offset) {
                    status = "ovulation";
                    break;
                }
            }
        }

        cells.push(CalendarDay {
            day: Some(d),
            date_str: cell_date.format("%Y-%m-%d").to_string(),
            status: status.to_string(),
            is_today: cell_date == today,
        });
    }

    cells
}

fn evaluate_phase(cycle_day: i64, avg_cycle: i64, ovulation_day: i64) -> (String, String) {
    if cycle_day < 1 {
        ("Scheduled".to_string(), "Upcoming logged date.".to_string())
    } else if cycle_day <= 5 {
        (
            format!("Menstrual Phase (Day {})", cycle_day),
            "Prioritize rest, warmth, and comfort food. Watch for cramps or fatigue.".to_string(),
        )
    } else if cycle_day < ovulation_day - 2 {
        (
            format!("Follicular Phase (Day {})", cycle_day),
            "Energy and mood are rebounding. Great window for active dates.".to_string(),
        )
    } else if cycle_day <= ovulation_day + 2 {
        (
            format!("Ovulation Window (Day {})", cycle_day),
            "Peak energy window (also highest fertility window).".to_string(),
        )
    } else if cycle_day <= avg_cycle - 5 {
        (
            format!("Early Luteal Phase (Day {})", cycle_day),
            "Energy gradually tapering down; keep routines steady.".to_string(),
        )
    } else if cycle_day <= avg_cycle {
        (
            format!("Late Luteal / PMS Window (Day {})", cycle_day),
            "Higher chance of PMS sensitivity, bloating, or cravings. Keep plans low-stress.".to_string(),
        )
    } else {
        (
            format!("Late ({} days overdue)", cycle_day - avg_cycle),
            "Cycle is running longer than average—check in gently without pressure.".to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ics_roundtrip() {
        let mut store = CycleStore::new(None);
        store.add_record(1, "2026-10-01", "Normal", "").unwrap();
        store.add_record(2, "2026-10-30", "Heavy", "Cravings").unwrap();

        let ics = store.generate_ics().unwrap();
        assert!(ics.contains("DTSTART;VALUE=DATE:20261123"));
        assert!(ics.contains("DTSTART;VALUE=DATE:20261128"));

        let mut restored = CycleStore::new(None);
        restored.import_from_ics(&ics).unwrap();
        assert_eq!(restored.records.len(), 2);
        assert_eq!(restored.records[0].notes, "Cravings");
    }
}