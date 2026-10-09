//! Visible raw cells only; formulas/caches are evidence, never execution authority.
use super::{
    docx::column_name,
    host::DetectedType,
    ooxml::{self, *},
};
use chrono::{Duration, NaiveDate};
use shixu_core::{
    contracts::{AppResult, error::AppError, notification::*},
    notifications::limits::Resource,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
pub fn extract_xlsx(_: &Path, _: &str, _: &ParserLimits) -> AppResult<PartResult> {
    Err(AppError::Unsupported)
}
pub fn extract_bytes(bytes: &[u8], id: &str, limits: &ParserLimits) -> AppResult<PartResult> {
    let mut output = Output::new(id)?;
    match parse(bytes, limits, &mut output) {
        Ok(()) => Ok(output.finish()),
        Err(e) => ooxml::failed(id, e),
    }
}
fn parse(bytes: &[u8], limits: &ParserLimits, output: &mut Output) -> Parse<()> {
    let mut package = Package::load(bytes, limits, DetectedType::Xlsx)?;
    let relations = package.relationships("xl/workbook.xml", limits)?;
    let root = package.xml("xl/workbook.xml", limits)?;
    if !root.is(S, "workbook") {
        return Err(PartReason::FormatUnsupported);
    }
    root.unique_children(S, &["workbookPr", "sheets"])?;
    let system = root
        .child(S, "workbookPr")
        .and_then(|n| n.attr("", "date1904"))
        .unwrap_or("0");
    let date1904 = boolean(system)?;
    let mut strings = vec![];
    let mut styles = vec![];
    let mut custom = BTreeMap::new();
    for kind in ["sharedStrings", "styles"] {
        let found: Vec<_> = relations.iter().filter(|r| r.kind == kind).collect();
        if found.len() > 1 {
            return Err(PartReason::FormatUnsupported);
        }
        if let Some(r) = found.first() {
            let name = r.target.as_deref().ok_or(PartReason::FormatUnsupported)?;
            package.require_type(name, if kind == "styles" { STYLES } else { STRINGS })?;
            let node = package.xml(name, limits)?;
            if kind == "sharedStrings" {
                if !node.is(S, "sst") {
                    return Err(PartReason::FormatUnsupported);
                }
                for item in &node.children {
                    if !item.is(S, "si") {
                        return Err(PartReason::FormatUnsupported);
                    }
                    strings.push(rich_text(item, output));
                }
            } else {
                if !node.is(S, "styleSheet") {
                    return Err(PartReason::FormatUnsupported);
                }
                node.unique_children(S, &["numFmts", "cellXfs", "cellStyleXfs"])?;
                if let Some(formats) = node.child(S, "numFmts") {
                    for n in &formats.children {
                        if !n.is(S, "numFmt") {
                            return Err(PartReason::FormatUnsupported);
                        }
                        let id = integer(n.attr("", "numFmtId"))?;
                        let code = n
                            .attr("", "formatCode")
                            .ok_or(PartReason::FormatUnsupported)?;
                        if custom.insert(id, code.to_owned()).is_some() {
                            return Err(PartReason::FormatUnsupported);
                        }
                    }
                }
                if let Some(xfs) = node.child(S, "cellXfs") {
                    for xf in &xfs.children {
                        if !xf.is(S, "xf") {
                            return Err(PartReason::FormatUnsupported);
                        }
                        let number_format = integer(xf.attr("", "numFmtId"))?;
                        let applied = xf.attr("", "applyNumberFormat").map(boolean).transpose()?;
                        let base = xf.attr("", "xfId").map(|v| integer(Some(v))).transpose()?;
                        // Disabled direct formats and unresolved inherited formats
                        // cannot establish dates in this deliberately small profile.
                        styles.push(
                            if applied == Some(false) || applied.is_none() && base.is_some() {
                                None
                            } else {
                                Some(number_format)
                            },
                        );
                    }
                }
            }
        }
    }
    let sheets = root
        .child(S, "sheets")
        .ok_or(PartReason::FormatUnsupported)?;
    limits.check(Resource::XlsxSheets, sheets.children.len() as u64)?;
    let mut used = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut sheet_ids = BTreeSet::new();
    let mut cells = 0u64;
    for n in &root.children {
        if !matches!(
            n.name.as_str(),
            "workbookPr" | "sheets" | "bookViews" | "definedNames" | "calcPr"
        ) || n.ns != S
        {
            output.partial = true
        }
        if n.is(S, "definedNames") {
            output.partial = true
        }
    }
    for sheet in &sheets.children {
        if !sheet.is(S, "sheet") {
            return Err(PartReason::FormatUnsupported);
        }
        let name = sheet
            .attr("", "name")
            .filter(|s| !s.is_empty())
            .ok_or(PartReason::FormatUnsupported)?;
        let sheet_id = integer(sheet.attr("", "sheetId"))?;
        if !names.insert(name) || sheet_id == 0 || !sheet_ids.insert(sheet_id) {
            return Err(PartReason::FormatUnsupported);
        }
        let state = sheet.attr("", "state").unwrap_or("visible");
        if !matches!(state, "visible" | "hidden" | "veryHidden") {
            return Err(PartReason::FormatUnsupported);
        }
        let hidden = state != "visible";
        output.partial |= hidden;
        let id = sheet.attr(R, "id").ok_or(PartReason::FormatUnsupported)?;
        let r = relations
            .iter()
            .find(|r| r.id == id && r.kind == "worksheet")
            .ok_or(PartReason::FormatUnsupported)?;
        let target = r.target.as_deref().ok_or(PartReason::FormatUnsupported)?;
        if !used.insert(target) {
            return Err(PartReason::FormatUnsupported);
        }
        package.require_type(target, SHEET)?;
        package.relationships(target, limits)?;
        let node = package.xml(target, limits)?;
        let context = SheetContext {
            name,
            hidden,
            date1904,
            strings: &strings,
            styles: &styles,
            custom: &custom,
        };
        read_sheet(&node, &context, limits, output, &mut cells)?;
    }
    output.partial |= package.partial;
    Ok(())
}
struct SheetContext<'a> {
    name: &'a str,
    hidden: bool,
    date1904: bool,
    strings: &'a [String],
    styles: &'a [Option<u32>],
    custom: &'a BTreeMap<u32, String>,
}
fn read_sheet(
    node: &Node,
    cx: &SheetContext<'_>,
    limits: &ParserLimits,
    output: &mut Output,
    count: &mut u64,
) -> Parse<()> {
    if !node.is(S, "worksheet") {
        return Err(PartReason::FormatUnsupported);
    }
    node.unique_children(
        S,
        &[
            "sheetData",
            "dimension",
            "cols",
            "mergeCells",
            "sheetFormatPr",
        ],
    )?;
    let default_hidden = node
        .child(S, "sheetFormatPr")
        .and_then(|p| p.attr("", "zeroHeight"))
        .map(boolean)
        .transpose()?
        .unwrap_or(false);
    let mut hidden_cols = BTreeSet::new();
    let mut merges = vec![];
    for n in &node.children {
        if n.is(S, "dimension") {
            let range = n.attr("", "ref").ok_or(PartReason::FormatUnsupported)?;
            range_bounds(range, limits)?;
        } else if n.is(S, "cols") {
            for col in &n.children {
                if !col.is(S, "col") {
                    return Err(PartReason::FormatUnsupported);
                }
                let min = integer(col.attr("", "min"))?;
                let max = integer(col.attr("", "max"))?;
                if min == 0 || max < min {
                    return Err(PartReason::FormatUnsupported);
                }
                limits.check(Resource::XlsxColumns, u64::from(max))?;
                if boolean(col.attr("", "hidden").unwrap_or("0"))? {
                    output.partial = true;
                    for c in min..=max {
                        hidden_cols.insert(c);
                    }
                }
            }
        } else if n.is(S, "mergeCells") {
            output.partial = true;
            for merge in &n.children {
                if !merge.is(S, "mergeCell") {
                    return Err(PartReason::FormatUnsupported);
                }
                merges.push(range_bounds(
                    merge.attr("", "ref").ok_or(PartReason::FormatUnsupported)?,
                    limits,
                )?);
            }
        } else if !n.is(S, "sheetData")
            && !n.is(S, "sheetFormatPr")
            && !n.is(S, "sheetViews")
            && !n.is(S, "pageMargins")
        {
            output.partial = true;
        }
    }
    let data = node
        .child(S, "sheetData")
        .ok_or(PartReason::FormatUnsupported)?;
    let mut rows = BTreeSet::new();
    let mut coords = BTreeSet::new();
    for row in &data.children {
        if !row.is(S, "row") {
            return Err(PartReason::FormatUnsupported);
        }
        let row_num = integer(row.attr("", "r"))?;
        if row_num == 0 || !rows.insert(row_num) {
            return Err(PartReason::FormatUnsupported);
        }
        limits.check(Resource::XlsxRows, u64::from(row_num))?;
        limits.check(Resource::XlsxRows, rows.len() as u64)?;
        let hidden = row
            .attr("", "hidden")
            .map(boolean)
            .transpose()?
            .unwrap_or(default_hidden);
        output.partial |= hidden;
        for cell in &row.children {
            if !cell.is(S, "c") {
                return Err(PartReason::FormatUnsupported);
            }
            let reference = cell.attr("", "r").ok_or(PartReason::FormatUnsupported)?;
            let (column, r) = coordinate(reference, limits)?;
            if r != row_num || !coords.insert(reference) {
                return Err(PartReason::FormatUnsupported);
            }
            cell.unique_children(S, &["f", "v", "is"])?;
            let formula = cell.child(S, "f").is_some();
            let value = cell.child(S, "v");
            let inline = cell.child(S, "is");
            if formula || value.is_some_and(|v| !v.text.is_empty()) || inline.is_some() {
                *count = count.saturating_add(1);
                limits.check(Resource::XlsxCells, *count)?;
            }
            let mut flags = vec![];
            if formula {
                output.partial = true;
                flags.extend([QualityFlag::FormulaDerived, QualityFlag::UncertainDate]);
            }
            for c in &cell.children {
                if !c.is(S, "f") && !c.is(S, "v") && !c.is(S, "is") {
                    output.partial = true;
                    flags.push(QualityFlag::PartialSource);
                }
            }
            let raw = value.map(|n| n.text.as_str()).unwrap_or("");
            let kind = cell.attr("", "t").unwrap_or("n");
            let text = match kind {
                "inlineStr" => rich_text(inline.ok_or(PartReason::FormatUnsupported)?, output),
                "s" => cx
                    .strings
                    .get(
                        raw.parse::<usize>()
                            .map_err(|_| PartReason::FormatUnsupported)?,
                    )
                    .ok_or(PartReason::FormatUnsupported)?
                    .clone(),
                "str" => raw.into(),
                "b" => if boolean(raw)? { "true" } else { "false" }.into(),
                "d" => {
                    if NaiveDate::parse_from_str(raw, "%Y-%m-%d").is_err() {
                        flags.push(QualityFlag::UncertainDate);
                        output.partial = true;
                    }
                    raw.into()
                }
                "e" => {
                    output.partial = true;
                    flags.push(QualityFlag::UncertainDate);
                    raw.into()
                }
                "n" => {
                    if raw.is_empty() {
                        String::new()
                    } else {
                        let style = cell
                            .attr("", "s")
                            .map(|s| {
                                s.parse::<usize>()
                                    .map_err(|_| PartReason::FormatUnsupported)
                            })
                            .transpose()?;
                        let fmt = style
                            .map(|i| {
                                cx.styles
                                    .get(i)
                                    .copied()
                                    .ok_or(PartReason::FormatUnsupported)
                            })
                            .transpose()?
                            .unwrap_or_else(|| cx.styles.first().copied().unwrap_or(Some(0)));
                        match fmt.and_then(|format| date_value(raw, format, cx.custom, cx.date1904))
                        {
                            Some(v) => v,
                            None => {
                                output.partial = true;
                                flags.push(QualityFlag::UncertainDate);
                                raw.into()
                            }
                        }
                    }
                }
                _ => return Err(PartReason::FormatUnsupported),
            };
            if cx.hidden || hidden || hidden_cols.contains(&column) {
                continue;
            }
            if merges
                .iter()
                .any(|&(c1, r1, c2, r2)| column >= c1 && column <= c2 && r >= r1 && r <= r2)
            {
                flags.push(QualityFlag::AmbiguousLayout);
            }
            output.push(
                text,
                PageOrSheet::Sheet {
                    name: cx.name.into(),
                },
                Some(EvidenceLocation::CellRange {
                    range: format!("{}{r}", column_name(column)),
                }),
                Method::Cell,
                flags,
                limits,
            )?;
        }
    }
    Ok(())
}
fn rich_text(n: &Node, output: &mut Output) -> String {
    let mut s = String::new();
    for c in &n.children {
        if c.is(S, "t") {
            s.push_str(&c.text)
        } else if c.is(S, "r") {
            for t in &c.children {
                if t.is(S, "t") {
                    s.push_str(&t.text)
                } else if !t.is(S, "rPr") {
                    output.partial = true
                }
            }
        } else {
            output.partial = true
        }
    }
    s
}
fn integer(s: Option<&str>) -> Parse<u32> {
    s.ok_or(PartReason::FormatUnsupported)?
        .parse()
        .map_err(|_| PartReason::FormatUnsupported)
}
fn boolean(s: &str) -> Parse<bool> {
    match s {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err(PartReason::FormatUnsupported),
    }
}
fn coordinate(s: &str, limits: &ParserLimits) -> Parse<(u32, u32)> {
    let split = s.bytes().take_while(u8::is_ascii_uppercase).count();
    if split == 0 || split == s.len() {
        return Err(PartReason::FormatUnsupported);
    }
    let mut col = 0u32;
    for c in s[..split].bytes() {
        col = col
            .checked_mul(26)
            .and_then(|n| n.checked_add(u32::from(c - b'A' + 1)))
            .ok_or(PartReason::LimitExceeded)?;
    }
    let tail = &s[split..];
    if tail.starts_with('0') || !tail.bytes().all(|b| b.is_ascii_digit()) {
        return Err(PartReason::FormatUnsupported);
    }
    let row: u32 = tail.parse().map_err(|_| PartReason::LimitExceeded)?;
    limits.check(Resource::XlsxColumns, u64::from(col))?;
    limits.check(Resource::XlsxRows, u64::from(row))?;
    Ok((col, row))
}
fn range_bounds(s: &str, limits: &ParserLimits) -> Parse<(u32, u32, u32, u32)> {
    let (a, b) = s.split_once(':').unwrap_or((s, s));
    let (c1, r1) = coordinate(a, limits)?;
    let (c2, r2) = coordinate(b, limits)?;
    if c2 < c1 || r2 < r1 {
        return Err(PartReason::FormatUnsupported);
    }
    Ok((c1, r1, c2, r2))
}
fn date_value(
    raw: &str,
    fmt: u32,
    custom: &BTreeMap<u32, String>,
    system1904: bool,
) -> Option<String> {
    // Deliberately small format grammar; localized/time-only/mixed codes remain raw/uncertain.
    let datetime = fmt == 22
        || custom
            .get(&fmt)
            .is_some_and(|s| matches!(s.as_str(), "yyyy-mm-dd hh:mm" | "yyyy/mm/dd hh:mm"));
    let date = matches!(fmt, 14..=17)
        || custom
            .get(&fmt)
            .is_some_and(|s| matches!(s.as_str(), "yyyy-mm-dd" | "yyyy/mm/dd" | "yyyy年mm月dd日"));
    if !date && !datetime {
        return None;
    }
    let serial: f64 = raw.parse().ok()?;
    if !serial.is_finite()
        || serial < if system1904 { 0.0 } else { 1.0 }
        || serial >= 2_958_466.0
        || !system1904 && serial.floor() == 60.0
    {
        return None;
    }
    let day = serial.floor() as i64;
    let offset = if system1904 {
        day
    } else if day > 60 {
        day - 1
    } else {
        day
    };
    let base = if system1904 {
        NaiveDate::from_ymd_opt(1904, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(1899, 12, 31)?
    };
    let date = base.checked_add_signed(Duration::days(offset))?;
    if date > NaiveDate::from_ymd_opt(9999, 12, 31)? {
        return None;
    }
    if datetime {
        let seconds = (serial.fract() * 86400.).round() as i64;
        if seconds >= 86400 {
            return None;
        }
        if seconds % 60 == 0 {
            Some(format!(
                "{} {:02}:{:02}",
                date,
                seconds / 3600,
                seconds % 3600 / 60
            ))
        } else {
            None
        }
    } else {
        Some(date.to_string())
    }
}
