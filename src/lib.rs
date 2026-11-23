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
    #[serde(default)]
    pub duration_days: Option<u32>, // None = 進行中 / 依歷史平均預估
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct RecordViewItem {
    pub id: u64,
    pub date: String,
    pub flow: String,
    pub notes: String,
    pub duration_days: Option<u32>,
    pub effective_duration: u32, // 實際天數（若未填則為 avg_duration）
    pub interval_days: Option<i64>,
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct CalendarDay {
    pub day: Option<u32>,
    pub date_str: String,
    pub status: String, // "none" | "logged" | "predicted" | "pms" | "ovulation"
    pub is_today: bool,
}

#[derive(Serialize, Tsify)]
#[tsify(into_wasm_abi)]
pub struct DashboardState {
    pub has_records: bool,
    pub avg_cycle: i64,
    pub avg_duration: u32,
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
        duration_days: Option<u32>,
    ) -> Result<String, JsValue> {
        NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
            .map_err(|_| JsValue::from_str("日期格式錯誤（需為 YYYY-MM-DD）"))?;

        if self.records.iter().any(|r| r.date == date_str) {
            return Err(JsValue::from_str("該日期已經有紀錄了"));
        }

        if let Some(d) = duration_days {
            if !(1..=15).contains(&d) {
                return Err(JsValue::from_str("行經天數需介於 1 到 15 天之間"));
            }
        }

        self.records.push(CycleRecord {
            id,
            date: date_str.to_string(),
            flow: flow.to_string(),
            notes: notes.to_string(),
            duration_days,
        });

        self.records.sort_by(|a, b| b.date.cmp(&a.date));
        self.export_snapshot()
    }

    /// 供歷史列表一鍵補登或修改單筆紀錄的行經天數（傳入 None 代表恢復為自動預估）
    pub fn update_duration(
        &mut self,
        id: u64,
        duration_days: Option<u32>,
    ) -> Result<String, JsValue> {
        if let Some(d) = duration_days {
            if !(1..=15).contains(&d) {
                return Err(JsValue::from_str("行經天數需介於 1 到 15 天之間"));
            }
        }

        let rec = self
            .records
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or_else(|| JsValue::from_str("找不到該筆紀錄"))?;

        rec.duration_days = duration_days;
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
            .map_err(|_| JsValue::from_str("無效的今日日期格式"))?;

        if self.records.is_empty() {
            let empty_grid = build_calendar_grid(view_year, view_month, today, &[], None, 28, 5);
            let empty = DashboardState {
                has_records: false,
                avg_cycle: 28,
                avg_duration: 5,
                cycle_day: None,
                phase_name: "尚無紀錄".to_string(),
                care_tip: "請先新增至少一筆經期開始日，系統將自動推算週期階段與照護提醒。".to_string(),
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

        let (avg_cycle, avg_duration, logged_spans, history_view) = self.compute_stats();
        let (last_start, last_duration) = logged_spans[0];
        let cycle_day = (today - last_start).num_days() + 1;
        let next_period = last_start + Duration::days(avg_cycle);
        let pms_start = next_period - Duration::days(5);
        let ovulation = next_period - Duration::days(14);
        let days_until_next = (next_period - today).num_days();

        let (phase_name, care_tip) =
            evaluate_phase(cycle_day, avg_cycle, avg_cycle - 14, last_duration as i64);

        let grid = build_calendar_grid(
            view_year,
            view_month,
            today,
            &logged_spans,
            Some(next_period),
            avg_cycle,
            avg_duration,
        );

        let state = DashboardState {
            has_records: true,
            avg_cycle,
            avg_duration,
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
            return Err(JsValue::from_str("目前沒有可匯出的紀錄"));
        }

        let (avg_cycle, avg_duration, logged_spans, _) = self.compute_stats();
        let (last_start, _) = logged_spans[0];
        let next_period = last_start + Duration::days(avg_cycle);
        let pms_start = next_period - Duration::days(5);
        let now_stamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();

        let snapshot_json = serde_json::to_string(&self.records)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let ics = format!(
            "BEGIN:VCALENDAR\r\n\
             VERSION:2.0\r\n\
             PRODID:-//MoonPhase//CycleEngine//ZH-HANT\r\n\
             CALSCALE:GREGORIAN\r\n\
             METHOD:PUBLISH\r\n\
             X-CYCLE-BACKUP:{}\r\n\
             BEGIN:VEVENT\r\n\
             UID:pms-{}@moonphase.local\r\n\
             DTSTAMP:{}\r\n\
             DTSTART;VALUE=DATE:{}\r\n\
             DTEND;VALUE=DATE:{}\r\n\
             SUMMARY:經前症候群 (PMS) 觀察期開始\r\n\
             DESCRIPTION:黃體期後期（預計經期前約 5 天），情緒與身體較敏感，宜保持行程輕鬆並準備舒適點心。\r\n\
             BEGIN:VALARM\r\n\
             ACTION:DISPLAY\r\n\
             DESCRIPTION:PMS 觀察期提醒\r\n\
             TRIGGER:PT9H\r\n\
             END:VALARM\r\n\
             END:VEVENT\r\n\
             BEGIN:VEVENT\r\n\
             UID:period-{}@moonphase.local\r\n\
             DTSTAMP:{}\r\n\
             DTSTART;VALUE=DATE:{}\r\n\
             DTEND;VALUE=DATE:{}\r\n\
             SUMMARY:預計生理期開始 (Day 1)\r\n\
             DESCRIPTION:依平均 {} 天週期推算（預計持續約 {} 天），請留意保暖並備妥熱飲與止痛藥。\r\n\
             BEGIN:VALARM\r\n\
             ACTION:DISPLAY\r\n\
             DESCRIPTION:明日預計生理期開始\r\n\
             TRIGGER:-PT15H\r\n\
             END:VALARM\r\n\
             BEGIN:VALARM\r\n\
             ACTION:DISPLAY\r\n\
             DESCRIPTION:今日預計生理期 Day 1\r\n\
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
            avg_cycle,
            avg_duration
        );

        Ok(ics)
    }

    pub fn import_from_ics(&mut self, ics_content: &str) -> Result<String, JsValue> {
        let unfolded = ics_content.replace("\r\n ", "").replace("\n ", "");
        let backup_line = unfolded
            .lines()
            .find(|line| line.starts_with("X-CYCLE-BACKUP:"))
            .ok_or_else(|| JsValue::from_str("此 .ics 檔案中找不到內嵌的備份資料"))?;

        let raw_json = backup_line.trim_start_matches("X-CYCLE-BACKUP:");
        let mut imported: Vec<CycleRecord> = serde_json::from_str(raw_json)
            .map_err(|e| JsValue::from_str(&format!("備份資料格式損毀：{}", e)))?;

        imported.sort_by(|a, b| b.date.cmp(&a.date));
        self.records = imported;
        self.export_snapshot()
    }
}

impl CycleStore {
    fn compute_stats(&self) -> (i64, u32, Vec<(NaiveDate, u32)>, Vec<RecordViewItem>) {
        let parsed_dates: Vec<NaiveDate> = self
            .records
            .iter()
            .filter_map(|r| NaiveDate::parse_from_str(&r.date, "%Y-%m-%d").ok())
            .collect();

        // 1. 計算平均行經天數（僅統計有明確填寫天數的紀錄）
        let mut total_dur = 0u32;
        let mut dur_count = 0u32;
        for rec in &self.records {
            if let Some(d) = rec.duration_days {
                if (1..=15).contains(&d) {
                    total_dur += d;
                    dur_count += 1;
                }
            }
        }
        let avg_duration = if dur_count > 0 {
            ((total_dur as f64) / (dur_count as f64)).round() as u32
        } else {
            5
        };

        // 2. 計算平均週期長度與每筆有效區間
        let mut total_diff = 0i64;
        let mut valid_intervals = 0i64;
        let mut history_view = Vec::with_capacity(self.records.len());
        let mut logged_spans = Vec::with_capacity(self.records.len());

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

            let eff_dur = rec.duration_days.unwrap_or(avg_duration);
            logged_spans.push((parsed_dates[i], eff_dur));

            history_view.push(RecordViewItem {
                id: rec.id,
                date: rec.date.clone(),
                flow: rec.flow.clone(),
                notes: rec.notes.clone(),
                duration_days: rec.duration_days,
                effective_duration: eff_dur,
                interval_days: interval,
            });
        }

        let avg_cycle = if valid_intervals > 0 {
            (total_diff as f64 / valid_intervals as f64).round() as i64
        } else {
            28
        };

        (avg_cycle, avg_duration, logged_spans, history_view)
    }
}

fn build_calendar_grid(
    year: i32,
    month: u32,
    today: NaiveDate,
    logged_spans: &[(NaiveDate, u32)],
    next_period_opt: Option<NaiveDate>,
    avg_cycle: i64,
    avg_duration: u32,
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

    let pred_dur = avg_duration as i64;

    for d in 1..=days_in_month {
        let cell_date = NaiveDate::from_ymd_opt(year, month, d).unwrap();
        let mut status = "none";

        let is_logged = logged_spans.iter().any(|&(start, dur)| {
            let offset = (cell_date - start).num_days();
            (0..(dur as i64)).contains(&offset)
        });

        if is_logged {
            status = "logged";
        } else {
            for &pred_start in &predicted_starts {
                let offset = (cell_date - pred_start).num_days();
                if (0..pred_dur).contains(&offset) {
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

fn evaluate_phase(
    cycle_day: i64,
    avg_cycle: i64,
    ovulation_day: i64,
    current_duration: i64,
) -> (String, String) {
    if cycle_day < 1 {
        (
            "已排程（尚未開始）".to_string(),
            "已預先記錄未來日期，屆時請留意身體狀況與保暖。".to_string(),
        )
    } else if cycle_day <= current_duration {
        (
            format!("行經期 (第 {} 天)", cycle_day),
            "身體較容易疲倦或經痛，建議準備熱飲、暖暖包與止痛藥，避免高強度或吹風行程。".to_string(),
        )
    } else if cycle_day < ovulation_day - 2 {
        (
            format!("濾泡期 (第 {} 天)", cycle_day),
            "精神與體力逐漸回升，情緒通常較穩定，適合安排戶外活動或運動約會。".to_string(),
        )
    } else if cycle_day <= ovulation_day + 2 {
        (
            format!("排卵期窗口 (第 {} 天)", cycle_day),
            "體力與活力處於週期高峰（同時為高受孕機率窗口，請留意防護措施）。".to_string(),
        )
    } else if cycle_day <= avg_cycle - 5 {
        (
            format!("黃體期前期 (第 {} 天)", cycle_day),
            "體力開始緩步下降，作息宜保持規律、避免過度熬夜。".to_string(),
        )
    } else if cycle_day <= avg_cycle {
        (
            format!("經前症候群 PMS 窗口 (第 {} 天)", cycle_day),
            "黃體素波動容易引發水腫、嗜甜或情緒敏感，多一點耐心傾聽，可準備她喜歡的點心。".to_string(),
        )
    } else {
        let overdue = cycle_day - avg_cycle;
        (
            format!("預計經期已延遲 {} 天 (第 {} 天)", overdue, cycle_day),
            "週期較平均值略長（壓力或作息改變皆可能影響），溫和關心即可，無需過度施壓。".to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dynamic_duration_and_legacy_compat() {
        // 測試向後相容：舊 JSON 沒有 duration_days 欄位
        let legacy_json = r#"[{"id":1,"date":"2026-10-01","flow":"正常","notes":""}]"#;
        let mut store = CycleStore::new(Some(legacy_json.to_string()));
        store
            .add_record(2, "2026-10-30", "偏多", "想吃甜食", Some(6))
            .unwrap();

        let (_, avg_dur, spans, _) = store.compute_stats();
        assert_eq!(avg_dur, 6);
        assert_eq!(spans[0].1, 6); // 新紀錄使用明確指定的 6 天
        assert_eq!(spans[1].1, 5); // 舊紀錄未填，但因整體僅有一筆有效天數 6，實際會動態套用 avg_duration=6
    }
}