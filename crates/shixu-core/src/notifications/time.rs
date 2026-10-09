//! Deterministic source-time parsing; never consults the OS clock or local zone.
use crate::contracts::{
    AppResult, UtcMillis,
    calendar::{Precision, TimeValue},
    error::AppError,
};
use chrono::{DateTime, Datelike, Days, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use regex::Regex;
use std::sync::OnceLock;
struct Patterns {
    date: Regex,
    clock: Regex,
    weekday: Regex,
}
fn patterns() -> &'static Patterns {
    static VALUE: OnceLock<Patterns> = OnceLock::new();
    VALUE.get_or_init(|| Patterns {
        date: Regex::new(r"(?:(\d{4})年)?(\d{1,2})月(\d{1,2})[日号]|(\d{4})[-/](\d{1,2})[-/](\d{1,2})").expect("static date grammar"),
        clock: Regex::new(r"(?:(上午|下午|晚上|中午|凌晨|早上)\s*)?(\d{1,2})(?:[:：](\d{2})|[点时](?:(\d{1,2})分?|(半))?)").expect("static clock grammar"),
        weekday: Regex::new(r"(?:星期|周)([一二三四五六日天])").expect("static weekday grammar"),
    })
}
pub(crate) fn unknown(text: &str, timezone: &str) -> TimeValue {
    TimeValue {
        precision: Precision::UnknownDate,
        local_date: None,
        start_at: None,
        end_at: None,
        timezone: timezone.into(),
        raw_time_text: text.into(),
    }
}
pub(crate) fn clear_time(value: &mut TimeValue) {
    value.precision = Precision::UnknownDate;
    value.local_date = None;
    value.start_at = None;
    value.end_at = None;
}
pub(crate) fn date_expression(text: &str) -> bool {
    patterns().date.is_match(text)
        || ["今天", "明天", "后天", "昨天", "下周", "近期", "周"]
            .iter()
            .any(|v| text.contains(v))
}
pub(crate) fn temporal_stripped(text: &str) -> String {
    let text = patterns().date.replace_all(text, "");
    let text = patterns().clock.replace_all(&text, "");
    let text = patterns().weekday.replace_all(&text, "");
    text.into_owned()
}
pub(crate) fn inherit_year(text: &str, year: &str) -> Option<String> {
    let captures: Vec<_> = patterns().date.captures_iter(text).collect();
    if captures.len() != 1
        || captures[0].get(1).is_some()
        || captures[0].get(4).is_some()
        || !complete_date_token(text, captures[0].get(0)?)
    {
        return None;
    }
    let found = captures[0].get(0)?;
    Some(format!(
        "{}{}年{}",
        &text[..found.start()],
        year,
        &text[found.start()..]
    ))
}
pub(crate) fn negated_change(text: &str) -> bool {
    static GRAMMAR: OnceLock<Regex> = OnceLock::new();
    GRAMMAR.get_or_init(||Regex::new(r"(?:不|未|没有|并非)(?:会|再|要|是|予以|进行|已|曾)*\s*(?:改至|改到|调整至|调整到|延期至)").expect("static negated change grammar")).is_match(text)
}
// A dot continues the captured numeric token only when followed by a digit.
// Sentence punctuation must not erase a fully grounded date/clock.
fn numeric_tail(tail: &str) -> bool {
    let mut chars = tail.chars();
    match chars.next() {
        Some(c) if c.is_numeric() => true,
        Some('.') => chars.next().is_some_and(char::is_numeric),
        _ => false,
    }
}
fn complete_date_token(text: &str, found: regex::Match<'_>) -> bool {
    let before = text[..found.start()].chars().next_back();
    let after = text[found.end()..].chars().next();
    !before.is_some_and(|c| c.is_numeric())
        && !(found.as_str().ends_with(|c: char| c.is_numeric())
            && (numeric_tail(&text[found.end()..]) || after.is_some_and(|c| c == '/' || c == '-')))
}
fn complete_clock_token(text: &str, found: regex::Match<'_>) -> bool {
    !text[..found.start()]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_numeric() || [':', '：', '.'].contains(&c))
        && !numeric_tail(&text[found.end()..])
        && !text[found.end()..]
            .chars()
            .next()
            .is_some_and(|c| [':', '：', '秒'].contains(&c))
}
/// `raw_time_text` is the unmodified source fragment (UTF-8), including ambiguity.
/// Invalid dates, competing dates, gap/fold instants and inverted ranges stay unknown.
pub fn parse_time(text: &str, sent_at: UtcMillis, timezone: &str) -> AppResult<TimeValue> {
    let tz: Tz = timezone.parse().map_err(|_| AppError::InvalidInput)?;
    let anchor = DateTime::<Utc>::from_timestamp_millis(sent_at)
        .ok_or(AppError::InvalidInput)?
        .with_timezone(&tz)
        .date_naive();
    let mut result = unknown(text, timezone);
    if negated_change(text) {
        return Ok(result);
    }
    let selected = ["改至", "改到", "调整至", "调整到", "延期至"]
        .iter()
        .filter_map(|marker| text.find(marker).map(|i| &text[i + marker.len()..]))
        .next()
        .unwrap_or(text);
    if ["大后天", "明天的明天", "可能", "也许", "或"]
        .iter()
        .any(|s| selected.contains(s))
    {
        return Ok(result);
    }
    let p = patterns();
    let dates: Vec<_> = p.date.captures_iter(selected).collect();
    if dates
        .iter()
        .any(|c| !complete_date_token(selected, c.get(0).expect("date capture")))
    {
        return Ok(result);
    }
    let relative: Vec<_> = [("今天", 0i64), ("明天", 1), ("后天", 2), ("昨天", -1)]
        .into_iter()
        .filter(|(word, _)| selected.contains(word))
        .collect();
    if dates.len() > 1 || relative.len() > 1 || (!dates.is_empty() && !relative.is_empty()) {
        return Ok(result);
    }
    let date = if let Some(c) = dates.first() {
        let (y, m, d) = if c.get(2).is_some() {
            (c.get(1), c.get(2), c.get(3))
        } else {
            (c.get(4), c.get(5), c.get(6))
        };
        let Some(y) = y.and_then(|v| v.as_str().parse::<i32>().ok()) else {
            return Ok(result);
        };
        let Some(date) = m
            .and_then(|v| v.as_str().parse::<u32>().ok())
            .zip(d.and_then(|v| v.as_str().parse::<u32>().ok()))
            .and_then(|(m, d)| NaiveDate::from_ymd_opt(y, m, d))
        else {
            return Ok(result);
        };
        date
    } else if let Some((_, offset)) = relative.first() {
        let date = if *offset >= 0 {
            anchor.checked_add_days(Days::new(*offset as u64))
        } else {
            anchor.checked_sub_days(Days::new(offset.unsigned_abs()))
        };
        let Some(date) = date else { return Ok(result) };
        date
    } else {
        return Ok(result);
    };
    for c in p.weekday.captures_iter(selected) {
        let weekday = match &c[1] {
            "一" => 1,
            "二" => 2,
            "三" => 3,
            "四" => 4,
            "五" => 5,
            "六" => 6,
            _ => 7,
        };
        if date.weekday().number_from_monday() != weekday {
            return Ok(result);
        }
    }
    result.local_date = Some(date.format("%Y-%m-%d").to_string());
    result.precision = Precision::DateOnly;
    let clocks: Vec<_> = p.clock.captures_iter(selected).collect();
    if clocks
        .iter()
        .any(|c| !complete_clock_token(selected, c.get(0).expect("clock capture")))
    {
        clear_time(&mut result);
        return Ok(result);
    }
    if clocks.is_empty() {
        if selected.contains("全天") || selected.contains("整天") {
            result.precision = Precision::ExplicitAllDay
        }
        return Ok(result);
    }
    if ["左右", "大约", "大概", "待定", "约"]
        .iter()
        .any(|s| selected.contains(s))
    {
        return Ok(result);
    }
    if clocks.len() > 2 {
        clear_time(&mut result);
        return Ok(result);
    }
    let parse_clock = |c: &regex::Captures<'_>| -> Option<NaiveTime> {
        let mut hour = c[2].parse::<u32>().ok()?;
        let minute = c
            .get(3)
            .or_else(|| c.get(4))
            .map(|s| s.as_str().parse::<u32>().ok())
            .unwrap_or(Some(if c.get(5).is_some() { 30 } else { 0 }))?;
        if let Some(period) = c.get(1) {
            if hour > 12 {
                return None;
            }
            match period.as_str() {
                "下午" | "晚上" if hour < 12 => hour += 12,
                "凌晨" | "上午" | "早上" if hour == 12 => hour = 0,
                "中午" if hour < 11 => hour += 12,
                _ => {}
            }
        }
        NaiveTime::from_hms_opt(hour, minute, 0)
    };
    let Some(start) =
        parse_clock(&clocks[0]).and_then(|t| tz.from_local_datetime(&date.and_time(t)).single())
    else {
        clear_time(&mut result);
        return Ok(result);
    };
    if let Some(c) = clocks.get(1) {
        let first = clocks[0].get(0).expect("whole capture");
        let second = c.get(0).expect("whole capture");
        let connector = &selected[first.end()..second.start()];
        if !["—", "–", "-", "~", "～", "至", "到"]
            .iter()
            .any(|s| connector.contains(s))
        {
            clear_time(&mut result);
            return Ok(result);
        }
        let end_date = if connector.contains("次日") || connector.contains("第二天") {
            date.checked_add_days(Days::new(1))
        } else {
            Some(date)
        };
        let Some(end) = end_date
            .zip(parse_clock(c))
            .and_then(|(d, t)| tz.from_local_datetime(&d.and_time(t)).single())
        else {
            clear_time(&mut result);
            return Ok(result);
        };
        if end <= start {
            clear_time(&mut result);
            return Ok(result);
        }
        result.end_at = Some(end.timestamp_millis());
    }
    result.precision = Precision::Exact;
    result.start_at = Some(start.timestamp_millis());
    Ok(result)
}
