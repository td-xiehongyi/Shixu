//! Bounded image observations. Native invocation requires an isolated adapter.
use shixu_core::contracts::{AppResult, error::AppError, notification::*};
use std::path::Path;
#[derive(Clone, Debug)]
pub struct Word {
    pub text: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub confidence: f32,
}
pub trait OcrEngine {
    fn version(&self) -> &str;
    fn recognize(&mut self, image: &::image::RgbImage) -> AppResult<Vec<Word>>;
}
pub fn extract_image(
    _input: &Path,
    _part_id: &str,
    _limits: &ParserLimits,
) -> AppResult<PartResult> {
    Err(AppError::Unsupported)
}
/// Portable decoder/observation algorithm. The caller supplies an explicitly
/// authorized engine; production must call this only inside the future sandbox.
pub fn extract_with_engine(
    bytes: &[u8],
    part_id: &str,
    limits: &ParserLimits,
    engine: &mut impl OcrEngine,
) -> AppResult<PartResult> {
    if bytes.len() as u64 > limits.max_image_bytes.min(10 * 1024 * 1024) {
        return failure(
            part_id,
            PartStatus::LimitExceeded,
            PartReason::LimitExceeded,
        );
    }
    let format = match ::image::guess_format(bytes) {
        Ok(f @ (::image::ImageFormat::Png | ::image::ImageFormat::Jpeg)) => f,
        _ => {
            return failure(
                part_id,
                PartStatus::Unsupported,
                PartReason::FormatUnsupported,
            );
        }
    };
    let dimensions =
        ::image::ImageReader::with_format(std::io::Cursor::new(bytes), format).into_dimensions();
    let (w, h) = match dimensions {
        Ok(size) => size,
        Err(_) => {
            return failure(
                part_id,
                PartStatus::RecognitionFailed,
                PartReason::RecognitionFailed,
            );
        }
    };
    if !valid_size(w, h, limits) {
        return failure(
            part_id,
            PartStatus::LimitExceeded,
            PartReason::LimitExceeded,
        );
    }
    let mut reader = ::image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut decode_limits = ::image::Limits::default();
    decode_limits.max_image_width = Some(limits.max_image_edge.min(10_000));
    decode_limits.max_image_height = Some(limits.max_image_edge.min(10_000));
    decode_limits.max_alloc = Some(limits.max_subprocess_memory_bytes.min(512 * 1024 * 1024));
    reader.limits(decode_limits);
    match reader.decode() {
        Ok(decoded) if decoded.width() == w && decoded.height() == h => {
            // Document OCR uses an explicit white background. Hidden RGB in a
            // transparent pixel must not become visible text when alpha is removed.
            let pixels = if decoded.color().has_alpha() {
                let rgba = decoded.into_rgba8();
                ::image::RgbImage::from_fn(w, h, |x, y| {
                    let p = rgba.get_pixel(x, y).0;
                    let alpha = u32::from(p[3]);
                    ::image::Rgb(std::array::from_fn(|i| {
                        ((u32::from(p[i]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
                    }))
                })
            } else {
                decoded.into_rgb8()
            };
            observe(&pixels, part_id, 1, limits, engine)
        }
        _ => failure(
            part_id,
            PartStatus::RecognitionFailed,
            PartReason::RecognitionFailed,
        ),
    }
}
pub(crate) fn valid_size(w: u32, h: u32, limits: &ParserLimits) -> bool {
    w > 0
        && h > 0
        && w <= limits.max_image_edge.min(10_000)
        && h <= limits.max_image_edge.min(10_000)
        && u64::from(w) * u64::from(h) <= limits.max_image_pixels.min(20_000_000)
}
pub(crate) fn failure(id: &str, status: PartStatus, reason: PartReason) -> AppResult<PartResult> {
    Ok(PartResult {
        part_id: id.parse().map_err(|_| AppError::InvalidInput)?,
        status,
        blocks: vec![],
        reason_code: Some(reason),
    })
}
pub(crate) fn valid_box(x: u32, y: u32, w: u32, h: u32, page_w: u32, page_h: u32) -> bool {
    w > 0
        && h > 0
        && x.checked_add(w).is_some_and(|end| end <= page_w)
        && y.checked_add(h).is_some_and(|end| end <= page_h)
}
/// Preserve spatial identity while assembling conservative line regions.
/// Confidence is evidence quality, not date interpretation.
pub fn observe(
    pixels: &::image::RgbImage,
    part_id: &str,
    page: u32,
    limits: &ParserLimits,
    engine: &mut impl OcrEngine,
) -> AppResult<PartResult> {
    let id = part_id.parse().map_err(|_| AppError::InvalidInput)?;
    if !valid_size(pixels.width(), pixels.height(), limits) || page == 0 {
        return failure(
            part_id,
            PartStatus::LimitExceeded,
            PartReason::LimitExceeded,
        );
    }
    let blurred = low_sharpness(pixels);
    let words = engine.recognize(pixels)?;
    let mut blocks = Vec::new();
    let mut chars = 0usize;
    let mut partial = false;
    for word in words {
        chars = chars.saturating_add(word.text.chars().count());
        if chars > limits.max_extracted_chars.min(200_000) as usize {
            return failure(
                part_id,
                PartStatus::LimitExceeded,
                PartReason::LimitExceeded,
            );
        }
        if !valid_box(
            word.x,
            word.y,
            word.width,
            word.height,
            pixels.width(),
            pixels.height(),
        ) || !word.confidence.is_finite()
            || !(0.0..=100.0).contains(&word.confidence)
            || word.text.chars().any(|c| c.is_control())
        {
            return failure(
                part_id,
                PartStatus::RecognitionFailed,
                PartReason::RecognitionFailed,
            );
        }
        if word.text.trim().is_empty() {
            continue;
        }
        let clipped = word.x <= 1
            || word.y <= 1
            || word.x + word.width >= pixels.width() - 1
            || word.y + word.height >= pixels.height() - 1;
        let weak = word.confidence < 85.0 || blurred;
        let mut flags = vec![];
        if weak || clipped {
            partial = true;
            flags.push(QualityFlag::PartialSource);
            // Ambiguous glyphs may include unreadable date digits: flag all weak
            // observations rather than guessing whether this was a date.
            flags.push(QualityFlag::UncertainDate);
        }
        blocks.push(EvidenceBlock {
            part_id: id,
            page_or_sheet: Some(PageOrSheet::Page { number: page }),
            cell_range_or_bbox: Some(EvidenceLocation::BoundingBox {
                x: word.x,
                y: word.y,
                width: word.width,
                height: word.height,
            }),
            text: word.text,
            method: Method::Ocr,
            engine_version: engine.version().into(),
            quality_flags: flags,
        });
    }
    if blocks.is_empty() {
        return failure(
            part_id,
            PartStatus::RecognitionFailed,
            PartReason::RecognitionFailed,
        );
    }
    let (blocks, ambiguous) = assemble(blocks);
    partial |= ambiguous;
    if blocks.iter().map(|b| b.text.chars().count()).sum::<usize>()
        > limits.max_extracted_chars.min(200_000) as usize
    {
        return failure(
            part_id,
            PartStatus::LimitExceeded,
            PartReason::LimitExceeded,
        );
    }
    Ok(PartResult {
        part_id: id,
        status: if partial {
            PartStatus::PartialParse
        } else {
            PartStatus::Success
        },
        blocks,
        reason_code: partial.then_some(PartReason::PartialSource),
    })
}

/// Conservative Laplacian energy over nonwhite pixels. This is a quality hint,
/// not a proof of readability; calibration/general quality acceptance remains
/// open. Unlike OCR confidence it also detects confident output from blurred ink.
fn low_sharpness(pixels: &::image::RgbImage) -> bool {
    if pixels.width() < 3 || pixels.height() < 3 {
        return true;
    }
    let gray = |x, y| {
        let p = pixels.get_pixel(x, y).0;
        (u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])) as i64 / 3
    };
    let (mut energy, mut count) = (0u64, 0u64);
    for y in 1..pixels.height() - 1 {
        for x in 1..pixels.width() - 1 {
            let c = gray(x, y);
            if c < 245 {
                let l = gray(x - 1, y) + gray(x + 1, y) + gray(x, y - 1) + gray(x, y + 1) - 4 * c;
                energy += (l * l) as u64;
                count += 1;
            }
        }
    }
    count == 0 || energy / count < 200
}

fn rect(block: &EvidenceBlock) -> (u32, u32, u32, u32) {
    match block.cell_range_or_bbox {
        Some(EvidenceLocation::BoundingBox {
            x,
            y,
            width,
            height,
        }) => (x, y, width, height),
        _ => unreachable!("validated raster observations"),
    }
}
/// Assemble spatially adjacent runs only. Large horizontal gaps retain separate
/// table/column regions. Overlapping/competing rows stay unjoined and uncertain.
/// Input is already bounded and shares one page and method.
pub(crate) fn assemble(mut input: Vec<EvidenceBlock>) -> (Vec<EvidenceBlock>, bool) {
    input.sort_by_key(|b| {
        let (x, y, _, _) = rect(b);
        (y, x)
    });
    let mut rows: Vec<Vec<EvidenceBlock>> = vec![];
    let mut ambiguous = false;
    for block in input {
        let (_, y, _, h) = rect(&block);
        let matches: Vec<_> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                let (_, ry, _, rh) = rect(&row[0]);
                let overlap = (y + h).min(ry + rh).saturating_sub(y.max(ry));
                overlap.saturating_mul(2) >= h.min(rh)
            })
            .map(|(i, _)| i)
            .collect();
        if matches.len() > 1 {
            ambiguous = true;
        }
        if let Some(i) = matches.first() {
            rows[*i].push(block);
        } else {
            rows.push(vec![block]);
        }
    }
    for row in &mut rows {
        row.sort_by_key(|b| rect(b).0);
    }
    let column_boundaries = recurring_gutters(&rows);
    let recurring_columns = column_boundaries.iter().any(|b| !b.is_empty());
    let mut result = vec![];
    for (row_index, row) in rows.into_iter().enumerate() {
        let overlap = row.windows(2).any(|b| {
            let (x, _, w, _) = rect(&b[0]);
            let (nx, _, nw, _) = rect(&b[1]);
            (x + w).saturating_sub(nx) > w.min(nw) / 4
        });
        if overlap || ambiguous {
            ambiguous = true;
            result.extend(row);
            continue;
        }
        let mut current: Option<EvidenceBlock> = None;
        let mut previous = (0, 0, 0, 0);
        for block in row {
            let (x, y, w, h) = rect(&block);
            if let Some(mut run) = current.take() {
                let (rx, ry, rw, rh) = rect(&run);
                let gap = x.saturating_sub(previous.0 + previous.2);
                if gap > 3 * previous.3.max(h) || column_boundaries[row_index].contains(&x) {
                    result.push(run);
                    current = Some(block);
                } else {
                    if run.method == Method::Ocr {
                        run.text.push(' ');
                    }
                    run.text.push_str(&block.text);
                    for flag in block.quality_flags {
                        if !run.quality_flags.contains(&flag) {
                            run.quality_flags.push(flag);
                        }
                    }
                    let top = y.min(ry);
                    let right = (x + w).max(rx + rw);
                    let bottom = (y + h).max(ry + rh);
                    run.cell_range_or_bbox = Some(EvidenceLocation::BoundingBox {
                        x: rx,
                        y: top,
                        width: right - rx,
                        height: bottom - top,
                    });
                    current = Some(run);
                }
            } else {
                current = Some(block);
            }
            previous = (x, y, w, h);
        }
        if let Some(run) = current {
            result.push(run);
        }
    }
    for block in &mut result {
        block.text = block.text.trim().to_owned();
    }
    ambiguous |= recurring_columns;
    if ambiguous {
        for block in &mut result {
            block.quality_flags.push(QualityFlag::AmbiguousLayout);
        }
    }
    (result, ambiguous)
}

/// A whitespace corridor at least one text height wide, recurring on different
/// rows with aligned right-hand starts, is evidence for competing column order.
/// Retain its boundary rather than interpreting row adjacency as a sentence.
/// Matching sorted neighboring candidates avoids a quadratic gap comparison.
fn recurring_gutters(rows: &[Vec<EvidenceBlock>]) -> Vec<std::collections::BTreeSet<u32>> {
    let mut boundaries = vec![std::collections::BTreeSet::new(); rows.len()];
    let mut gaps = vec![];
    for (row_index, row) in rows.iter().enumerate() {
        for pair in row.windows(2) {
            let (x, _, w, h) = rect(&pair[0]);
            let (next_x, _, _, next_h) = rect(&pair[1]);
            let height = h.max(next_h);
            if next_x.saturating_sub(x + w) >= height {
                gaps.push((row_index, x + w, next_x, height));
            }
        }
    }
    gaps.sort_unstable_by_key(|gap| gap.2);
    for pair in gaps.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let height = a.3.max(b.3);
        let shared_width = a.2.min(b.2).saturating_sub(a.1.max(b.1));
        if a.0 != b.0 && a.2.abs_diff(b.2) <= height / 2 && shared_width >= height {
            boundaries[a.0].insert(a.2);
            boundaries[b.0].insert(b.2);
        }
    }
    boundaries
}
