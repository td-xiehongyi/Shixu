use shixu_core::contracts::{error::AppError, notification::*};
use shixu_native::attachments::{
    image::{self, OcrEngine, Word},
    pdf,
};
const ID: &str = "00000000-0000-0000-0000-000000000004";
struct Observations(Vec<Word>);
impl OcrEngine for Observations {
    fn version(&self) -> &str {
        "controlled-observation/1"
    }
    fn recognize(&mut self, _: &::image::RgbImage) -> Result<Vec<Word>, AppError> {
        Ok(self.0.clone())
    }
}
fn word(text: &str, x: u32, confidence: f32) -> Word {
    Word {
        text: text.into(),
        x,
        y: 10,
        width: 70,
        height: 20,
        confidence,
    }
}
#[test]
fn image_text_and_simple_table_locations() {
    let pixels = ::image::RgbImage::new(500, 100);
    let mut engine = Observations(vec![
        word("Workshop", 10, 99.),
        word("2026-11-20", 250, 99.),
    ]);
    let result = image::observe(&pixels, ID, 1, &ParserLimits::default(), &mut engine).unwrap();
    assert_eq!(result.blocks.len(), 2);
    assert_eq!(result.blocks[0].text, "Workshop");
    assert_eq!(
        result.blocks[1].cell_range_or_bbox,
        Some(EvidenceLocation::BoundingBox {
            x: 250,
            y: 10,
            width: 70,
            height: 20
        })
    );
    assert!(result.blocks.iter().all(
        |b| b.method == Method::Ocr && b.page_or_sheet == Some(PageOrSheet::Page { number: 1 })
    ));
}
#[test]
fn low_quality_date_is_flagged() {
    let mut engine = Observations(vec![word("2026-11-20", 10, 35.)]);
    let result = image::observe(
        &::image::RgbImage::new(500, 100),
        ID,
        1,
        &ParserLimits::default(),
        &mut engine,
    )
    .unwrap();
    assert!(
        result.blocks[0]
            .quality_flags
            .contains(&QualityFlag::UncertainDate)
    );
    assert_eq!(result.status, PartStatus::PartialParse);
}
#[test]
fn malformed_and_oversize_image_are_rejected_by_rust() {
    let mut engine = Observations(vec![]);
    let result =
        image::extract_with_engine(b"not an image", ID, &ParserLimits::default(), &mut engine)
            .unwrap();
    assert_eq!(result.status, PartStatus::Unsupported);
    let limits = ParserLimits {
        max_image_bytes: 3,
        ..ParserLimits::default()
    };
    assert_eq!(
        image::extract_with_engine(b"1234", ID, &limits, &mut engine)
            .unwrap()
            .status,
        PartStatus::LimitExceeded
    );
    let bad = Observations(vec![word("outside", 490, 99.)]);
    assert_eq!(
        image::observe(
            &::image::RgbImage::new(500, 100),
            ID,
            1,
            &ParserLimits::default(),
            &mut { bad }
        )
        .unwrap()
        .status,
        PartStatus::RecognitionFailed
    );
}
#[test]
fn product_entry_points_remain_closed() {
    let p = std::path::Path::new("does-not-exist");
    assert_eq!(
        image::extract_image(p, ID, &ParserLimits::default()),
        Err(AppError::Unsupported)
    );
    assert_eq!(
        pdf::extract_pdf(p, ID, &ParserLimits::default()),
        Err(AppError::Unsupported)
    );
}
struct Pages(u32);
impl pdf::PdfDocument for Pages {
    fn page_count(&self) -> u32 {
        self.0
    }
    fn page_size(&self, _: u32) -> Result<(u32, u32), AppError> {
        Ok((500, 100))
    }
    fn version(&self) -> &str {
        "controlled-pdf/1"
    }
    fn native_text(&self, p: u32, _: u32) -> Result<Vec<pdf::TextRegion>, AppError> {
        Ok(if p == 0 {
            vec![pdf::TextRegion {
                text: "Native notice".into(),
                x: 10,
                y: 10,
                width: 100,
                height: 20,
            }]
        } else {
            vec![]
        })
    }
    fn render(&self, _: u32, w: u32, h: u32) -> Result<::image::RgbImage, AppError> {
        Ok(::image::RgbImage::new(w, h))
    }
}
#[test]
fn mixed_pdf_uses_text_or_ocr_per_page() {
    let mut engine = Observations(vec![word("Scan", 10, 99.)]);
    let mixed =
        pdf::extract_document(&Pages(2), ID, &ParserLimits::default(), &mut engine).unwrap();
    assert_eq!(mixed.blocks[0].method, Method::NativeText);
    assert_eq!(mixed.blocks[1].method, Method::Ocr);
    assert_eq!(
        mixed.blocks[1].page_or_sheet,
        Some(PageOrSheet::Page { number: 2 })
    );
}
#[test]
fn twenty_first_page_is_rejected() {
    let mut engine = Observations(vec![]);
    assert_eq!(
        pdf::extract_document(&Pages(21), ID, &ParserLimits::default(), &mut engine)
            .unwrap()
            .status,
        PartStatus::LimitExceeded
    );
}
#[path = "support/image_pdf_engines.rs"]
mod engines;
fn normalized(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}
fn check_fixture(result: &PartResult, fixture: &serde_json::Value) -> (usize, usize, usize, usize) {
    let name = fixture["file"].as_str().unwrap();
    match fixture["expected"].as_str().unwrap() {
        "readable" => assert!(!result.blocks.is_empty(), "{name}: {:?}", result.status),
        "degraded" => assert!(
            matches!(
                result.status,
                PartStatus::PartialParse | PartStatus::RecognitionFailed
            ),
            "{name}: {:?}",
            result.status
        ),
        "limit_exceeded" => assert_eq!(result.status, PartStatus::LimitExceeded, "{name}"),
        "recognition_failed" => assert_eq!(result.status, PartStatus::RecognitionFailed, "{name}"),
        "unsupported" => assert_eq!(result.status, PartStatus::Unsupported, "{name}"),
        other => panic!("unexpected expectation {other}"),
    }
    let regions = fixture["regions"].as_array().unwrap();
    let mut complete = 0;
    let mut located = 0;
    for region in regions {
        let b = region["bbox"].as_array().unwrap();
        let coords: Vec<_> = b.iter().map(|x| x.as_u64().unwrap() as u32).collect();
        let page = region["page"].as_u64().unwrap_or(1) as u32;
        let local = result
            .blocks
            .iter()
            .filter(|block| {
                block.page_or_sheet == Some(PageOrSheet::Page { number: page })
                    && match block.cell_range_or_bbox {
                        Some(EvidenceLocation::BoundingBox {
                            x,
                            y,
                            width,
                            height,
                        }) => {
                            x >= coords[0]
                                && y >= coords[1]
                                && x + width <= coords[2]
                                && y + height <= coords[3]
                        }
                        _ => false,
                    }
            })
            .map(|b| b.text.as_str())
            .collect::<String>();
        if normalized(&local).contains(&normalized(region["text"].as_str().unwrap())) {
            complete += 1;
        }
    }
    for block in &result.blocks {
        assert!(!block.engine_version.is_empty());
        assert!(!block.text.contains('<'));
        if regions.iter().any(|r| {
            let a = r["bbox"].as_array().unwrap();
            let c: Vec<_> = a.iter().map(|x| x.as_u64().unwrap() as u32).collect();
            block.page_or_sheet
                == Some(PageOrSheet::Page {
                    number: r["page"].as_u64().unwrap_or(1) as u32,
                })
                && match block.cell_range_or_bbox {
                    Some(EvidenceLocation::BoundingBox {
                        x,
                        y,
                        width,
                        height,
                    }) => x >= c[0] && y >= c[1] && x + width <= c[2] && y + height <= c[3],
                    _ => false,
                }
        }) {
            located += 1;
        }
    }
    println!(
        "QUALITY {name}: status={:?} complete_regions={complete}/{} located_blocks={located}/{}",
        result.status,
        regions.len(),
        result.blocks.len()
    );
    let eligible = if fixture["expected"] == "limit_exceeded" {
        0
    } else {
        regions.len()
    };
    (complete, eligible, located, result.blocks.len())
}
#[test]
fn twenty_real_image_fixtures() {
    let _lock = engines::NATIVE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    engines::verify_engines();
    let manifest = engines::manifest("image");
    let cases = manifest["fixtures"].as_array().unwrap();
    assert!(cases.len() >= 20);
    let (mut complete, mut total, mut located, mut blocks) = (0, 0, 0, 0);
    for case in cases {
        let bytes = engines::approved_bytes("image", case["file"].as_str().unwrap());
        let result = image::extract_with_engine(
            &bytes,
            ID,
            &ParserLimits::default(),
            &mut engines::Tesseract,
        )
        .unwrap();
        let (a, b, c, d) = check_fixture(&result, case);
        complete += a;
        total += b;
        located += c;
        blocks += d;
    }
    println!(
        "IMAGE TOTAL fixtures={} complete_regions={complete}/{total} located_blocks={located}/{blocks}",
        cases.len()
    );
}
#[test]
fn twenty_real_pdf_fixtures() {
    let _lock = engines::NATIVE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let engine = engines::Pdfium::new();
    let manifest = engines::manifest("pdf");
    let cases = manifest["fixtures"].as_array().unwrap();
    assert!(cases.len() >= 20);
    let (mut complete, mut total, mut located, mut blocks) = (0, 0, 0, 0);
    for case in cases {
        let name = case["file"].as_str().unwrap();
        let bytes = engines::approved_bytes("pdf", name);
        let result = pdf::extract_with_reader(
            &bytes,
            ID,
            &ParserLimits::default(),
            &engine,
            &mut engines::Tesseract,
        )
        .unwrap();
        if case["expected"] == "auth_required" {
            assert_eq!(result.reason_code, Some(PartReason::AuthRequired));
            assert!(result.blocks.is_empty());
            println!("QUALITY {name}: AuthRequired");
            continue;
        }
        if name == "mixed.pdf" {
            assert_eq!(result.blocks[0].method, Method::NativeText);
            assert!(result.blocks.iter().any(|b| b.method == Method::Ocr
                && b.page_or_sheet == Some(PageOrSheet::Page { number: 2 })));
        }
        let (a, b, c, d) = check_fixture(&result, case);
        complete += a;
        total += b;
        located += c;
        blocks += d;
    }
    println!(
        "PDF TOTAL fixtures={} complete_regions={complete}/{total} located_blocks={located}/{blocks}",
        cases.len()
    );
}
#[test]
fn pdf_actions_never_execute() {
    let _lock = engines::NATIVE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let engine = engines::Pdfium::new();
    let path = std::path::Path::new("/tmp/shixu-n4-must-not-open");
    assert!(!path.exists());
    for name in ["actions.pdf", "embedded.pdf", "xfa.pdf"] {
        let doc = engine.open(engines::approved_bytes("pdf", name)).unwrap();
        let result =
            pdf::extract_document(&doc, ID, &ParserLimits::default(), &mut engines::Tesseract)
                .unwrap();
        assert_eq!(result.status, PartStatus::Success);
        assert!(!path.exists());
    }
    // This verifies no canary creation. OS-observed file/network-denial evidence
    // is explicitly a separate blocked Windows gate, not implied by this test.
}
#[test]
fn coherent_ocr_lines_preserve_column_boundaries() {
    let mut engine = Observations(vec![
        word("Workshop", 10, 99.),
        word("Monday", 95, 99.),
        word("Lecture", 350, 99.),
    ]);
    let result = image::observe(
        &::image::RgbImage::new(500, 100),
        ID,
        1,
        &ParserLimits::default(),
        &mut engine,
    )
    .unwrap();
    assert_eq!(result.blocks.len(), 2);
    assert_eq!(result.blocks[0].text, "Workshop Monday");
    assert_eq!(result.blocks[1].text, "Lecture");
}
#[test]
fn native_pdf_has_coherent_notice_regions() {
    let _lock = engines::NATIVE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let engine = engines::Pdfium::new();
    let doc = engine
        .open(engines::approved_bytes("pdf", "native.pdf"))
        .unwrap();
    let result =
        pdf::extract_document(&doc, ID, &ParserLimits::default(), &mut engines::Tesseract).unwrap();
    assert!(
        result
            .blocks
            .iter()
            .any(|b| b.text == "Workshop 2026-11-20"),
        "{:?}",
        result.blocks
    );
}
#[test]
fn overlapping_observations_are_not_joined_into_false_sentences() {
    let mut engine = Observations(vec![word("Workshop", 10, 99.), word("Cancelled", 30, 99.)]);
    let result = image::observe(
        &::image::RgbImage::new(500, 100),
        ID,
        1,
        &ParserLimits::default(),
        &mut engine,
    )
    .unwrap();
    assert_eq!(result.blocks.len(), 2);
    assert!(
        result
            .blocks
            .iter()
            .all(|b| b.quality_flags.contains(&QualityFlag::AmbiguousLayout))
    );
    assert_eq!(result.status, PartStatus::PartialParse);
}
#[test]
fn character_budget_includes_inserted_word_spaces() {
    let mut engine = Observations(vec![word("Workshop", 10, 99.), word("Room", 95, 99.)]);
    let limits = ParserLimits {
        max_extracted_chars: 12,
        ..ParserLimits::default()
    };
    assert_eq!(
        image::observe(
            &::image::RgbImage::new(500, 100),
            ID,
            1,
            &limits,
            &mut engine
        )
        .unwrap()
        .status,
        PartStatus::LimitExceeded
    );
}
struct Reader(std::cell::Cell<u32>);
impl pdf::PdfReader for Reader {
    type Document<'a> = Pages;
    fn open(&self, _: &[u8]) -> Result<Pages, AppError> {
        self.0.set(self.0.get() + 1);
        Ok(Pages(1))
    }
}
#[test]
fn pdf_byte_limits_and_magic_are_checked_before_engine_open() {
    let reader = Reader(std::cell::Cell::new(0));
    let mut engine = Observations(vec![]);
    let limits = ParserLimits {
        max_file_bytes: 8,
        ..ParserLimits::default()
    };
    assert_eq!(
        pdf::extract_with_reader(b"%PDF-1.7\n", ID, &limits, &reader, &mut engine)
            .unwrap()
            .status,
        PartStatus::LimitExceeded
    );
    assert_eq!(reader.0.get(), 0);
    assert_eq!(
        pdf::extract_with_reader(b"garbage", ID, &limits, &reader, &mut engine)
            .unwrap()
            .status,
        PartStatus::Unsupported
    );
    assert_eq!(reader.0.get(), 0);
}
