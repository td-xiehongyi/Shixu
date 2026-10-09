//! Per-page PDF algorithm; no document actions, forms or attachment API.
use super::image::OcrEngine;
use shixu_core::contracts::{AppResult, error::AppError, notification::*};
use std::path::Path;
#[derive(Clone, Debug)]
pub struct TextRegion {
    /// Preserve native whitespace in text; glyph ink gaps cannot infer spaces.
    pub text: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
pub trait PdfDocument {
    fn page_count(&self) -> u32;
    fn page_size(&self, page: u32) -> AppResult<(u32, u32)>;
    fn native_text(&self, page: u32, max_chars: u32) -> AppResult<Vec<TextRegion>>;
    fn render(&self, page: u32, width: u32, height: u32) -> AppResult<::image::RgbImage>;
    fn version(&self) -> &str;
}
pub fn extract_pdf(_input: &Path, _part_id: &str, _limits: &ParserLimits) -> AppResult<PartResult> {
    Err(AppError::Unsupported)
}
/// Read each bounded page independently. Text-bearing pages retain native text;
/// only pages without usable native text are rendered. Coordinates are pixels
/// at the adapter's declared page size, top-left origin for both methods.
pub fn extract_document(
    doc: &impl PdfDocument,
    part_id: &str,
    limits: &ParserLimits,
    ocr: &mut impl OcrEngine,
) -> AppResult<PartResult> {
    use super::image::{failure, observe, valid_box, valid_size};
    let id = part_id.parse().map_err(|_| AppError::InvalidInput)?;
    let pages = doc.page_count();
    if pages == 0 {
        return failure(
            part_id,
            PartStatus::RecognitionFailed,
            PartReason::RecognitionFailed,
        );
    }
    if pages > limits.max_pdf_pages.min(20) {
        return failure(
            part_id,
            PartStatus::LimitExceeded,
            PartReason::LimitExceeded,
        );
    }
    let mut blocks = vec![];
    let mut partial = false;
    let mut chars = 0usize;
    let cap = limits.max_extracted_chars.min(200_000) as usize;
    for page in 0..pages {
        let (w, h) = doc.page_size(page)?;
        if !valid_size(w, h, limits) {
            return failure(
                part_id,
                PartStatus::LimitExceeded,
                PartReason::LimitExceeded,
            );
        }
        let regions = doc.native_text(page, (cap - chars) as u32)?;
        if regions.iter().all(|r| r.text.trim().is_empty()) {
            let pixels = doc.render(page, w, h)?;
            if pixels.width() != w || pixels.height() != h {
                return Err(AppError::ParseFailed);
            }
            let result = observe(&pixels, part_id, page + 1, limits, ocr)?;
            if result.status == PartStatus::LimitExceeded {
                return Ok(result);
            }
            partial |= result.status != PartStatus::Success;
            for b in result.blocks {
                chars = chars.saturating_add(b.text.chars().count());
                blocks.push(b);
            }
        } else {
            let mut native_blocks = vec![];
            for r in regions {
                chars = chars.saturating_add(r.text.chars().count());
                if !valid_box(r.x, r.y, r.width, r.height, w, h)
                    || r.text.chars().any(|c| c.is_control())
                {
                    return failure(
                        part_id,
                        PartStatus::RecognitionFailed,
                        PartReason::RecognitionFailed,
                    );
                }
                if r.text.trim().is_empty() {
                    continue;
                }
                native_blocks.push(EvidenceBlock {
                    part_id: id,
                    page_or_sheet: Some(PageOrSheet::Page { number: page + 1 }),
                    cell_range_or_bbox: Some(EvidenceLocation::BoundingBox {
                        x: r.x,
                        y: r.y,
                        width: r.width,
                        height: r.height,
                    }),
                    text: r.text,
                    method: Method::NativeText,
                    engine_version: doc.version().into(),
                    quality_flags: vec![],
                });
            }
            let (native_blocks, ambiguous) = super::image::assemble(native_blocks);
            partial |= ambiguous;
            blocks.extend(native_blocks);
            chars = blocks.iter().map(|b| b.text.chars().count()).sum();
        }
        if chars > cap {
            return failure(
                part_id,
                PartStatus::LimitExceeded,
                PartReason::LimitExceeded,
            );
        }
    }
    if blocks.is_empty() {
        return failure(
            part_id,
            PartStatus::RecognitionFailed,
            PartReason::RecognitionFailed,
        );
    }
    if partial {
        for b in &mut blocks {
            if !b.quality_flags.contains(&QualityFlag::PartialSource) {
                b.quality_flags.push(QualityFlag::PartialSource);
            }
        }
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
/// Reader ports must expose text/rendering only and bound allocations before
/// returning observations. This interface grants no process or filesystem access.
pub trait PdfReader {
    type Document<'a>: PdfDocument
    where
        Self: 'a;
    fn open<'a>(&'a self, bytes: &[u8]) -> AppResult<Self::Document<'a>>;
}
pub fn extract_with_reader(
    bytes: &[u8],
    part_id: &str,
    limits: &ParserLimits,
    reader: &impl PdfReader,
    ocr: &mut impl OcrEngine,
) -> AppResult<PartResult> {
    use super::image::failure;
    if bytes.len() as u64 > limits.max_file_bytes.min(20 * 1024 * 1024) {
        return failure(
            part_id,
            PartStatus::LimitExceeded,
            PartReason::LimitExceeded,
        );
    }
    if !bytes.starts_with(b"%PDF-") {
        return failure(
            part_id,
            PartStatus::Unsupported,
            PartReason::FormatUnsupported,
        );
    }
    match reader.open(bytes) {
        Ok(doc) => extract_document(&doc, part_id, limits, ocr),
        Err(AppError::AuthFailed) => failure(
            part_id,
            PartStatus::RecognitionFailed,
            PartReason::AuthRequired,
        ),
        Err(AppError::Unsupported) => Err(AppError::Unsupported),
        Err(_) => failure(
            part_id,
            PartStatus::RecognitionFailed,
            PartReason::RecognitionFailed,
        ),
    }
}
