use shixu_core::contracts::{error::AppError, notification::*};
use shixu_native::attachments::{docx, xlsx};
use std::{
    io::{Cursor, Write},
    path::Path,
};
const ID: &str = "00000000-0000-0000-0000-000000000004";
const ROOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/attachments/"
);
fn manifest(fmt: &str) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(format!("{ROOT}{fmt}_manifest.json")).unwrap()).unwrap()
}
fn fixture(fmt: &str, id: &str) -> Vec<u8> {
    let m = manifest(fmt);
    let f = m["fixtures"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == id)
        .unwrap();
    std::fs::read(format!("{ROOT}{}", f["file"].as_str().unwrap())).unwrap()
}
fn parse(fmt: &str, bytes: &[u8], limits: &ParserLimits) -> PartResult {
    match fmt {
        "docx" => docx::extract_bytes(bytes, ID, limits),
        "xlsx" => xlsx::extract_bytes(bytes, ID, limits),
        _ => unreachable!(),
    }
    .unwrap()
}
fn result(fmt: &str, id: &str) -> PartResult {
    parse(fmt, &fixture(fmt, id), &ParserLimits::default())
}
fn text(r: &PartResult) -> String {
    r.blocks
        .iter()
        .map(|b| b.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}
fn rewrite(
    bytes: &[u8],
    transform: impl Fn(&str, Vec<u8>) -> Vec<u8>,
    add: &[(&str, &[u8])],
    compression: zip::CompressionMethod,
) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(compression);
    for i in 0..archive.len() {
        let mut e = archive.by_index(i).unwrap();
        let name = e.name().unwrap().into_owned();
        let mut v = Vec::new();
        std::io::Read::read_to_end(&mut e, &mut v).unwrap();
        out.start_file(&name, options).unwrap();
        out.write_all(&transform(&name, v)).unwrap();
    }
    for (n, b) in add {
        out.start_file(*n, options).unwrap();
        out.write_all(b).unwrap();
    }
    out.finish().unwrap().into_inner()
}
#[test]
fn docx_paragraph_table_and_inline_image() {
    let p = result("docx", "paragraph");
    assert_eq!(p.status, PartStatus::Success);
    assert_eq!(text(&p), "2026年10月12日9:00高数考试");
    assert_eq!(
        p.blocks[0].page_or_sheet,
        Some(PageOrSheet::Paragraph { number: 1 })
    );
    let table = result("docx", "table");
    assert_eq!(table.blocks.len(), 2);
    assert_eq!(
        table.blocks[0].cell_range_or_bbox,
        Some(EvidenceLocation::CellRange {
            range: "table1!A1".into()
        })
    );
}
#[test]
fn tracked_changes_and_floating_text_are_partial() {
    for id in [
        "tracked",
        "deleted",
        "floating",
        "textbox",
        "columns",
        "object",
        "field",
        "headers_omitted",
        "nested_table",
        "hidden_run",
    ] {
        let r = result("docx", id);
        assert_eq!(r.status, PartStatus::PartialParse, "{id}");
        assert!(
            r.blocks
                .iter()
                .all(|b| b.quality_flags.contains(&QualityFlag::PartialSource)),
            "{id}"
        );
        if !["columns"].contains(&id) {
            assert!(!text(&r).contains("高数考试"), "{id}");
        }
    }
}
#[test]
fn xlsx_visible_cells_keep_sheet_coordinates() {
    let r = result("xlsx", "coordinates");
    assert_eq!(r.status, PartStatus::Success);
    assert_eq!(
        r.blocks[0].page_or_sheet,
        Some(PageOrSheet::Sheet {
            name: "通知".into()
        })
    );
    assert_eq!(
        r.blocks[0].cell_range_or_bbox,
        Some(EvidenceLocation::CellRange { range: "B1".into() })
    );
    let r = result("xlsx", "merged");
    assert_eq!(r.status, PartStatus::PartialParse);
    assert!(
        r.blocks[0]
            .quality_flags
            .contains(&QualityFlag::AmbiguousLayout)
    );
}
#[test]
fn formula_cache_never_decides_date() {
    for id in ["formula", "formula_date"] {
        let r = result("xlsx", id);
        assert_eq!(r.status, PartStatus::PartialParse);
        assert!(
            r.blocks[0]
                .quality_flags
                .contains(&QualityFlag::FormulaDerived)
        );
        assert!(
            r.blocks[0]
                .quality_flags
                .contains(&QualityFlag::UncertainDate)
        );
    }
}
#[test]
fn date_system_and_format_are_respected() {
    for id in ["date1900", "date1904", "custom_format"] {
        let r = result("xlsx", id);
        assert_eq!(r.status, PartStatus::Success, "{id}");
        assert_eq!(text(&r), "2026-10-12", "{id}");
    }
    for id in ["serial60", "numeric", "ambiguous_format"] {
        let r = result("xlsx", id);
        assert_eq!(r.status, PartStatus::PartialParse, "{id}");
        assert!(
            r.blocks[0]
                .quality_flags
                .contains(&QualityFlag::UncertainDate)
        );
    }
}
#[test]
fn hidden_cells_are_counted_not_extracted() {
    for id in ["hidden_row", "hidden_column", "hidden_sheet"] {
        let r = result("xlsx", id);
        assert_eq!(r.status, PartStatus::PartialParse);
        assert!(r.blocks.is_empty());
        let limits = ParserLimits {
            max_xlsx_nonempty_cells: 0,
            ..ParserLimits::default()
        };
        assert_eq!(
            parse("xlsx", &fixture("xlsx", id), &limits).status,
            PartStatus::LimitExceeded,
            "{id}"
        );
    }
}
#[test]
fn macro_external_link_and_zip_bomb_are_inert() {
    for fmt in ["docx", "xlsx"] {
        for id in ["macro", "wrong_namespace", "dtd"] {
            let r = result(fmt, id);
            assert_eq!(r.status, PartStatus::Unsupported, "{fmt}/{id}");
            assert!(r.blocks.is_empty());
        }
    }
    assert_eq!(result("docx", "hyperlink").status, PartStatus::PartialParse);
    assert_eq!(result("xlsx", "external").status, PartStatus::PartialParse);
    let bomb = rewrite(
        &fixture("docx", "paragraph"),
        |_, v| v,
        &[("padding.xml", &vec![b' '; 1_000_000])],
        zip::CompressionMethod::Deflated,
    );
    assert_eq!(
        parse("docx", &bomb, &ParserLimits::default()).status,
        PartStatus::LimitExceeded
    );
}
#[test]
fn namespaces_entity_escapes_and_rich_strings() {
    assert_eq!(
        text(&result("docx", "renamed_prefix")),
        "2026年10月12日9:00高数考试"
    );
    for fmt in ["docx", "xlsx"] {
        assert_eq!(text(&result(fmt, "escaped")), "A&B <通知>");
    }
    assert_eq!(
        text(&result("xlsx", "shared_strings")),
        "2026年10月12日9:00高数考试"
    );
}
#[test]
fn output_characters_and_archive_limits_tighten_only() {
    let bytes = fixture("docx", "paragraph");
    for limits in [
        ParserLimits {
            max_extracted_chars: 2,
            ..ParserLimits::default()
        },
        ParserLimits {
            max_file_bytes: 2,
            ..ParserLimits::default()
        },
        ParserLimits {
            max_uncompressed_bytes: 2,
            ..ParserLimits::default()
        },
        ParserLimits {
            max_archive_entries: 2,
            ..ParserLimits::default()
        },
        ParserLimits {
            max_compression_ratio: 0,
            ..ParserLimits::default()
        },
    ] {
        assert_eq!(
            parse("docx", &bytes, &limits).status,
            PartStatus::LimitExceeded
        );
    }
    let huge = rewrite(
        &bytes,
        |n, v| {
            if n == "word/document.xml" {
                String::from_utf8(v)
                    .unwrap()
                    .replace("2026年10月12日9:00高数考试", &"字".repeat(200001))
                    .into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    let limits = ParserLimits {
        max_extracted_chars: u32::MAX,
        ..ParserLimits::default()
    };
    assert_eq!(
        parse("docx", &huge, &limits).status,
        PartStatus::LimitExceeded
    );
}
#[test]
fn xlsx_row_column_bounds_include_hidden_and_declared_dimensions() {
    let bytes = fixture("xlsx", "visible");
    for replacement in [
        "<row r=\"2001\" hidden=\"1\">",
        "<row r=\"1\"><c r=\"AY1\" t=\"inlineStr\">",
        "<row r=\"1\"><c r=\"A2001\" t=\"inlineStr\">",
    ] {
        let changed = rewrite(
            &bytes,
            |n, v| {
                if n.ends_with("sheet1.xml") {
                    let s = String::from_utf8(v).unwrap();
                    if replacement.starts_with("<row r=\"2001") {
                        s.replace("<row r=\"1\" >", replacement).into_bytes()
                    } else {
                        s.replace("<row r=\"1\" ><c r=\"A1\" t=\"inlineStr\">", replacement)
                            .into_bytes()
                    }
                } else {
                    v
                }
            },
            &[],
            zip::CompressionMethod::Stored,
        );
        let limits = ParserLimits {
            max_xlsx_rows_per_sheet: u32::MAX,
            max_xlsx_columns_per_sheet: u32::MAX,
            ..ParserLimits::default()
        };
        assert_eq!(
            parse("xlsx", &changed, &limits).status,
            PartStatus::LimitExceeded,
            "{replacement}"
        );
    }
    for limits in [
        ParserLimits {
            max_xlsx_sheets: 0,
            ..ParserLimits::default()
        },
        ParserLimits {
            max_xlsx_rows_per_sheet: 0,
            ..ParserLimits::default()
        },
        ParserLimits {
            max_xlsx_columns_per_sheet: 0,
            ..ParserLimits::default()
        },
        ParserLimits {
            max_xlsx_nonempty_cells: 0,
            ..ParserLimits::default()
        },
    ] {
        assert_eq!(
            parse("xlsx", &bytes, &limits).status,
            PartStatus::LimitExceeded
        );
    }
}
#[test]
fn tentative_content_is_discarded_when_late_member_fails() {
    let bytes = rewrite(
        &fixture("docx", "paragraph"),
        |_, v| v,
        &[("vba.bin", b"never-execute")],
        zip::CompressionMethod::Stored,
    );
    let r = parse("docx", &bytes, &ParserLimits::default());
    assert_eq!(r.status, PartStatus::Unsupported);
    assert!(r.blocks.is_empty());
}
#[test]
fn path_based_production_and_binary_remain_closed() {
    let path = Path::new("/never-open-this-file");
    assert_eq!(
        docx::extract_docx(path, ID, &ParserLimits::default()),
        Err(AppError::Unsupported)
    );
    assert_eq!(
        xlsx::extract_xlsx(path, ID, &ParserLimits::default()),
        Err(AppError::Unsupported)
    );
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_shixu-parser"))
        .arg("--docx")
        .arg(path)
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(78));
}
#[test]
fn per_format_fixture_manifest_gold_is_preserved() {
    use sha2::{Digest, Sha256};
    let mut failures = vec![];
    for fmt in ["docx", "xlsx"] {
        let m = manifest(fmt);
        let cases = m["fixtures"].as_array().unwrap();
        assert!(cases.len() >= 20);
        for f in cases {
            let bytes = fixture(fmt, f["id"].as_str().unwrap());
            assert_eq!(
                Sha256::digest(&bytes)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
                f["sha256"].as_str().unwrap()
            );
            let r = parse(fmt, &bytes, &ParserLimits::default());
            if text(&r) != f["expected_extracted"]
                || format!("{:?}", r.status) != f["status"].as_str().unwrap()
            {
                failures.push(format!("{fmt}/{}: {:?} {:?}", f["id"], r.status, text(&r)));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
fn inline_image_bytes() -> Vec<u8> {
    let mut png = Cursor::new(Vec::new());
    ::image::DynamicImage::ImageRgb8(::image::RgbImage::from_fn(200, 80, |x, y| {
        if (x + y) % 2 == 0 {
            ::image::Rgb([0, 0, 0])
        } else {
            ::image::Rgb([255, 255, 255])
        }
    }))
    .write_to(&mut png, ::image::ImageFormat::Png)
    .unwrap();
    let rel=br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image.png"/></Relationships>"#;
    rewrite(
        &fixture("docx", "paragraph"),
        |n, v| {
            if n == "word/document.xml" {
                String::from_utf8(v).unwrap().replace("</w:body>","<w:p><w:r><w:drawing><wp:inline><a:graphic><a:blip r:embed=\"img\"/></a:graphic></wp:inline></w:drawing></w:r></w:p></w:body>").into_bytes()
            } else {
                v
            }
        },
        &[
            ("word/media/image.png", png.get_ref()),
            ("word/_rels/document.xml.rels", rel),
        ],
        zip::CompressionMethod::Stored,
    )
}
#[test]
fn docx_inline_image_port_preserves_ocr_quality_and_budget_without_engine() {
    use shixu_native::attachments::image::{OcrEngine, Word};
    struct Controlled {
        calls: usize,
    }
    impl OcrEngine for Controlled {
        fn version(&self) -> &str {
            "synthetic-observation-only"
        }
        fn recognize(&mut self, _: &::image::RgbImage) -> Result<Vec<Word>, AppError> {
            self.calls += 1;
            Ok(vec![Word {
                text: "2026年10月13日英语考试".into(),
                x: 10,
                y: 10,
                width: 160,
                height: 20,
                confidence: 50.,
            }])
        }
    }
    let bytes = inline_image_bytes();
    let no_engine = parse("docx", &bytes, &ParserLimits::default());
    assert_eq!(no_engine.status, PartStatus::PartialParse);
    assert_eq!(no_engine.blocks.len(), 1);
    let mut engine = Controlled { calls: 0 };
    let observed =
        docx::extract_with_engine(&bytes, ID, &ParserLimits::default(), &mut engine).unwrap();
    assert_eq!(engine.calls, 1);
    assert_eq!(observed.blocks.len(), 2);
    assert_eq!(observed.status, PartStatus::PartialParse);
    let b = &observed.blocks[1];
    assert_eq!(b.method, Method::Ocr);
    assert_eq!(b.engine_version, "synthetic-observation-only");
    assert_eq!(b.page_or_sheet, Some(PageOrSheet::Paragraph { number: 2 }));
    assert!(b.quality_flags.contains(&QualityFlag::UncertainDate));
    let limits = ParserLimits {
        max_image_bytes: 0,
        ..ParserLimits::default()
    };
    assert_eq!(
        parse("docx", &bytes, &limits).status,
        PartStatus::LimitExceeded
    );
    let mut jpeg = Cursor::new(Vec::new());
    ::image::DynamicImage::new_rgb8(200, 80)
        .write_to(&mut jpeg, ::image::ImageFormat::Jpeg)
        .unwrap();
    let jpeg_bytes = rewrite(
        &bytes,
        |n, v| {
            if n == "[Content_Types].xml" {
                String::from_utf8(v)
                    .unwrap()
                    .replace("image/png", "image/jpeg")
                    .into_bytes()
            } else if n == "word/media/image.png" {
                jpeg.get_ref().clone()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    let mut jpeg_engine = Controlled { calls: 0 };
    let jpeg_result =
        docx::extract_with_engine(&jpeg_bytes, ID, &ParserLimits::default(), &mut jpeg_engine)
            .unwrap();
    assert_eq!(jpeg_engine.calls, 1);
    assert_eq!(jpeg_result.blocks.len(), 2);
    assert_eq!(jpeg_result.status, PartStatus::PartialParse);
    let aggregate_limit = ParserLimits {
        max_extracted_chars: no_engine.blocks[0].text.chars().count() as u32,
        ..ParserLimits::default()
    };
    let r = docx::extract_with_engine(&bytes, ID, &aggregate_limit, &mut engine).unwrap();
    assert_eq!(r.status, PartStatus::LimitExceeded);
    assert!(r.blocks.is_empty());
}
#[test]
fn paragraph_columns_and_unvisited_table_content_are_partial() {
    let bytes = rewrite(
        &fixture("docx", "paragraph"),
        |n, v| {
            if n == "word/document.xml" {
                String::from_utf8(v)
                    .unwrap()
                    .replace(
                        "<w:p>",
                        "<w:p><w:pPr><w:sectPr><w:cols w:num=\"2\"/></w:sectPr></w:pPr>",
                    )
                    .into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    assert_eq!(
        parse("docx", &bytes, &ParserLimits::default()).status,
        PartStatus::PartialParse
    );
    let bytes = rewrite(
        &fixture("docx", "table"),
        |n, v| {
            if n == "word/document.xml" {
                String::from_utf8(v)
                    .unwrap()
                    .replace(
                        "<w:tr>",
                        "<w:customXml><w:p><w:r><w:t>遗漏</w:t></w:r></w:p></w:customXml><w:tr>",
                    )
                    .into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    assert_eq!(
        parse("docx", &bytes, &ParserLimits::default()).status,
        PartStatus::PartialParse
    );
}
#[test]
fn actual_xlsx_sheet_cell_and_archive_entry_ceilings_cannot_be_raised() {
    let source = fixture("xlsx", "visible");
    let mut sheet_names = String::new();
    let mut rels = String::new();
    let mut types = String::new();
    let mut extra_owned = vec![];
    for i in 2..=11 {
        sheet_names.push_str(&format!(
            "<sheet name=\"hidden{i}\" sheetId=\"{i}\" r:id=\"s{i}\" state=\"hidden\"/>"
        ));
        rels.push_str(&format!("<Relationship Id=\"s{i}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{i}.xml\"/>"));
        types.push_str(&format!("<Override PartName=\"/xl/worksheets/sheet{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"));
        extra_owned.push((format!("xl/worksheets/sheet{i}.xml"),br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#.to_vec()));
    }
    let extra: Vec<_> = extra_owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let sheets = rewrite(
        &source,
        |n, v| {
            let mut s = String::from_utf8(v).unwrap();
            if n == "xl/workbook.xml" {
                s = s.replace("</sheets>", &format!("{sheet_names}</sheets>"));
            }
            if n == "xl/_rels/workbook.xml.rels" {
                s = s.replace("</Relationships>", &format!("{rels}</Relationships>"));
            }
            if n == "[Content_Types].xml" {
                s = s.replace("</Types>", &format!("{types}</Types>"));
            }
            s.into_bytes()
        },
        &extra,
        zip::CompressionMethod::Stored,
    );
    let raised = ParserLimits {
        max_xlsx_sheets: u32::MAX,
        max_xlsx_nonempty_cells: u32::MAX,
        max_archive_entries: u32::MAX,
        ..ParserLimits::default()
    };
    assert_eq!(
        parse("xlsx", &sheets, &raised).status,
        PartStatus::LimitExceeded
    );
    let ten = rewrite(
        &sheets,
        |n, v| {
            if n == "xl/workbook.xml" {
                String::from_utf8(v)
                    .unwrap()
                    .replace(
                        "<sheet name=\"hidden11\" sheetId=\"11\" r:id=\"s11\" state=\"hidden\"/>",
                        "",
                    )
                    .into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    assert_eq!(
        parse("xlsx", &ten, &raised).status,
        PartStatus::PartialParse
    );
    let mut data = String::new();
    for r in 1..=401 {
        data.push_str(&format!("<row r=\"{r}\" hidden=\"1\">"));
        for c in 1..=if r == 401 { 1 } else { 50 } {
            data.push_str(&format!(
                "<c r=\"{}{r}\" t=\"inlineStr\"><is><t>x</t></is></c>",
                column(c)
            ));
        }
        data.push_str("</row>");
    }
    let cells = rewrite(
        &source,
        |n, v| {
            if n.ends_with("sheet1.xml") {
                format!("<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>{data}</sheetData></worksheet>").into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    assert_eq!(
        parse("xlsx", &cells, &raised).status,
        PartStatus::LimitExceeded
    );
    let boundary = rewrite(
        &cells,
        |n, v| {
            if n.ends_with("sheet1.xml") {
                String::from_utf8(v).unwrap().replace("<row r=\"401\" hidden=\"1\"><c r=\"A401\" t=\"inlineStr\"><is><t>x</t></is></c></row>","").into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    assert_eq!(
        parse("xlsx", &boundary, &raised).status,
        PartStatus::PartialParse
    );
    let additions: Vec<_> = (0..4998)
        .map(|i| (format!("inert{i}.dat"), vec![0]))
        .collect();
    let extra: Vec<_> = additions
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let many = rewrite(
        &fixture("docx", "paragraph"),
        |_, v| v,
        &extra,
        zip::CompressionMethod::Stored,
    );
    assert_eq!(
        parse("docx", &many, &raised).status,
        PartStatus::LimitExceeded
    );
    let exact = rewrite(
        &fixture("docx", "paragraph"),
        |_, v| v,
        &extra[..4997],
        zip::CompressionMethod::Stored,
    );
    assert_eq!(parse("docx", &exact, &raised).status, PartStatus::Success);
}
fn column(mut n: u32) -> String {
    let mut s = String::new();
    while n > 0 {
        n -= 1;
        s.insert(0, char::from(b'A' + (n % 26) as u8));
        n /= 26;
    }
    s
}
#[test]
fn xml_entities_depth_relationships_and_file_bytes_fail_closed() {
    let source = fixture("docx", "paragraph");
    for replacement in [
        "&notdefined;",
        "&#0;",
        "<foreign xmlns=\"urn:test\">&notdefined;</foreign>",
    ] {
        let bytes = rewrite(
            &source,
            |n, v| {
                if n == "word/document.xml" {
                    String::from_utf8(v)
                        .unwrap()
                        .replace("2026年10月12日9:00高数考试", replacement)
                        .into_bytes()
                } else {
                    v
                }
            },
            &[],
            zip::CompressionMethod::Stored,
        );
        let r = parse("docx", &bytes, &ParserLimits::default());
        assert_eq!(r.status, PartStatus::Unsupported);
        assert!(r.blocks.is_empty());
    }
    let depth = rewrite(
        &source,
        |n, v| {
            if n == "word/document.xml" {
                String::from_utf8(v)
                    .unwrap()
                    .replace(
                        "2026年10月12日9:00高数考试",
                        &format!("{}text{}", "<deep>".repeat(65), "</deep>".repeat(65)),
                    )
                    .into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    assert_eq!(
        parse("docx", &depth, &ParserLimits::default()).status,
        PartStatus::LimitExceeded
    );
    let unsafe_rel = rewrite(
        &source,
        |n, v| {
            if n == "_rels/.rels" {
                String::from_utf8(v)
                    .unwrap()
                    .replace("word/document.xml", "../../escape.xml")
                    .into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    assert_eq!(
        parse("docx", &unsafe_rel, &ParserLimits::default()).status,
        PartStatus::Unsupported
    );
    let padded = rewrite(
        &source,
        |_, v| v,
        &[("inert.dat", &vec![0; 20 * 1024 * 1024])],
        zip::CompressionMethod::Stored,
    );
    let raised = ParserLimits {
        max_file_bytes: u64::MAX,
        ..ParserLimits::default()
    };
    assert_eq!(
        parse("docx", &padded, &raised).status,
        PartStatus::LimitExceeded
    );
}
#[test]
fn exact_character_row_column_boundaries_and_1900_edges_are_preserved() {
    let source = fixture("docx", "paragraph");
    let chars = rewrite(
        &source,
        |n, v| {
            if n == "word/document.xml" {
                String::from_utf8(v)
                    .unwrap()
                    .replace("2026年10月12日9:00高数考试", &"字".repeat(200000))
                    .into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    let r = parse("docx", &chars, &ParserLimits::default());
    assert_eq!(r.status, PartStatus::Success);
    assert_eq!(r.blocks[0].text.chars().count(), 200000);
    let source = fixture("xlsx", "visible");
    let corner = rewrite(
        &source,
        |n, v| {
            if n.ends_with("sheet1.xml") {
                String::from_utf8(v)
                    .unwrap()
                    .replace("r=\"1\"", "r=\"2000\"")
                    .replace("r=\"A1\"", "r=\"AX2000\"")
                    .into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    let r = parse("xlsx", &corner, &ParserLimits::default());
    assert_eq!(r.status, PartStatus::Success);
    assert_eq!(
        r.blocks[0].cell_range_or_bbox,
        Some(EvidenceLocation::CellRange {
            range: "AX2000".into()
        })
    );
    for (serial, expected) in [
        ("1", "1900-01-01"),
        ("59", "1900-02-28"),
        ("61", "1900-03-01"),
    ] {
        let bytes = rewrite(
            &fixture("xlsx", "date1900"),
            |n, v| {
                if n.ends_with("sheet1.xml") {
                    String::from_utf8(v)
                        .unwrap()
                        .replace("46307", serial)
                        .into_bytes()
                } else {
                    v
                }
            },
            &[],
            zip::CompressionMethod::Stored,
        );
        assert_eq!(
            text(&parse("xlsx", &bytes, &ParserLimits::default())),
            expected
        );
    }
    for serial in ["-1", "NaN", "inf", "2958466", "60.5"] {
        let bytes = rewrite(
            &fixture("xlsx", "date1900"),
            |n, v| {
                if n.ends_with("sheet1.xml") {
                    String::from_utf8(v)
                        .unwrap()
                        .replace("46307", serial)
                        .into_bytes()
                } else {
                    v
                }
            },
            &[],
            zip::CompressionMethod::Stored,
        );
        let r = parse("xlsx", &bytes, &ParserLimits::default());
        assert_eq!(r.status, PartStatus::PartialParse);
        assert_eq!(text(&r), serial);
    }
}
fn lcs(a: &str, b: &str) -> u64 {
    let a: Vec<_> = a.chars().collect();
    let b: Vec<_> = b.chars().collect();
    let mut dp = vec![0; b.len() + 1];
    for ca in a {
        let mut last = 0;
        for (i, cb) in b.iter().enumerate() {
            let old = dp[i + 1];
            dp[i + 1] = if ca == *cb {
                last + 1
            } else {
                dp[i].max(dp[i + 1])
            };
            last = old;
        }
    }
    dp[b.len()]
}
fn metric(tp: u64, fp: u64, fn_: u64) -> serde_json::Value {
    serde_json::json!({"tp":tp,"fp":fp,"fn":fn_,"precision":if tp+fp==0{None}else{Some(tp as f64/(tp+fp)as f64)},"recall":if tp+fn_==0{None}else{Some(tp as f64/(tp+fn_)as f64)}})
}
#[test]
fn per_format_character_semantic_and_durable_calendar_metrics() {
    use shixu_core::{
        calendar::EventService,
        contracts::{
            AppResult,
            calendar::{EventQuery, Precision},
        },
        notifications::{
            MessageStore, aggregate::merge_parts, extract::extract, identity::message_identity,
        },
        storage::{DataProtector, Database},
    };
    use std::sync::Arc;
    use uuid::Uuid;
    struct SyntheticProtector;
    impl DataProtector for SyntheticProtector {
        fn protect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
            Ok(p.iter().map(|b| b ^ 0xa5).collect())
        }
        fn unprotect(&self, p: &[u8]) -> AppResult<Vec<u8>> {
            self.protect(p)
        }
    }
    let mut formats = serde_json::Map::new();
    for fmt in ["docx", "xlsx"] {
        let path = std::env::temp_dir().join(format!("shixu-n5-metrics-{}.db", Uuid::new_v4()));
        let protector = Arc::new(SyntheticProtector);
        let db = Arc::new(Database::open(&path, protector.clone()).unwrap());
        let store = MessageStore::new(db.clone());
        let service = EventService::new(db.clone());
        let config = SourceConfig {
            source_id: SourceId::from_uuid(Uuid::new_v4()),
            adapter_type: "synthetic".into(),
            account_id: "test".into(),
            allowed_group_ids: vec!["fixture".into()],
            timezone: "Asia/Shanghai".into(),
            enabled: true,
            capability_set: vec![
                SourceCapability::LiveMessages,
                SourceCapability::Attachments,
            ],
        };
        store.bind_source(&config).unwrap();
        let mut char_counts = [0u64; 3];
        let mut semantic = [0u64; 3];
        let mut calendar = [0u64; 3];
        let mut rows = vec![];
        let mut statuses = std::collections::BTreeMap::new();
        let mut ambiguous_guesses = 0;
        let m = manifest(fmt);
        for f in m["fixtures"].as_array().unwrap() {
            let id = f["id"].as_str().unwrap();
            let r = result(fmt, id);
            *statuses.entry(format!("{:?}", r.status)).or_insert(0u64) += 1;
            let output = text(&r);
            let source = f["source_gold"].as_str().unwrap();
            let correct = lcs(source, &output);
            let chars = [
                correct,
                output.chars().count() as u64 - correct,
                source.chars().count() as u64 - correct,
            ];
            for i in 0..3 {
                char_counts[i] += chars[i];
            }
            let key = MessageKey::from_uuid(Uuid::new_v4());
            let part = MessagePart {
                part_id: r.part_id,
                message_key: key,
                kind: PartKind::File,
                source_file_ref: None,
                original_name: Some(format!("{id}.{fmt}")),
                declared_type: None,
                detected_type: Some(fmt.into()),
                byte_size: None,
                content_hash: None,
                fetch_state: FetchState::Fetched,
                parse_state: PartStatus::Fetched,
                failure_code: None,
                encrypted_blob_ref: None,
                retained_until: None,
            };
            let mut msg = MessageEnvelope {
                message_key: key,
                source_id: config.source_id,
                account_id: config.account_id.clone(),
                group_id: "fixture".into(),
                native_message_id: id.into(),
                sent_at: 1791532800000,
                received_at: 1791532800000,
                sender_id: "synthetic".into(),
                text: String::new(),
                reply_to: None,
                revision: 1,
                revoked: false,
                processing_state: ProcessingState::Persisted,
                parts: vec![part],
            };
            msg.message_key = message_identity(&config, &msg).unwrap().key;
            msg.parts[0].message_key = msg.message_key;
            store.append(&config, msg.clone()).unwrap();
            store
                .record_parts(&msg.message_key, 1, vec![r.clone()])
                .unwrap();
            let body = extract(&msg, &[], &[], &config.timezone).unwrap();
            let batch = merge_parts(&body, std::slice::from_ref(&r)).unwrap();
            let gold = f["events"].as_array().unwrap();
            let mut remaining: Vec<_> = gold.iter().collect();
            let mut sem = [0u64; 3];
            for c in &batch.candidates {
                if let Some(i) = remaining.iter().position(|g| g["title"] == c.title) {
                    remaining.remove(i);
                    sem[0] += 1;
                } else {
                    sem[1] += 1;
                }
            }
            sem[2] = remaining.len() as u64;
            for i in 0..3 {
                semantic[i] += sem[i];
            }
            let before = service
                .query(EventQuery {
                    from_date: None,
                    through_date: None,
                    statuses: vec![],
                    include_pending: true,
                })
                .unwrap()
                .len();
            let summary = service.apply(batch.clone()).unwrap();
            assert_eq!(service.apply(batch.clone()).unwrap().created, 0);
            let events = service
                .query(EventQuery {
                    from_date: None,
                    through_date: None,
                    statuses: vec![],
                    include_pending: true,
                })
                .unwrap();
            assert_eq!(events.len() - before, summary.created as usize);
            let event_ids: Vec<_> = service
                .notices()
                .unwrap()
                .into_iter()
                .filter(|s| s.message_key == msg.message_key)
                .filter_map(|s| s.event_id)
                .collect();
            let actual: Vec<_> = events
                .iter()
                .filter(|e| event_ids.contains(&e.event_id))
                .collect();
            let mut remaining: Vec<_> = gold.iter().collect();
            let mut cal = [0u64; 3];
            for e in &actual {
                if let Some(i) = remaining.iter().position(|g| {
                    g["title"] == e.title
                        && g["precision"] == format!("{:?}", e.time_precision)
                        && g["local_date"] == serde_json::to_value(&e.local_date).unwrap()
                }) {
                    remaining.remove(i);
                    cal[0] += 1;
                } else {
                    cal[1] += 1;
                }
            }
            cal[2] = remaining.len() as u64;
            for i in 0..3 {
                calendar[i] += cal[i];
            }
            if r.blocks.iter().any(|b| {
                b.quality_flags.contains(&QualityFlag::FormulaDerived)
                    || b.quality_flags.contains(&QualityFlag::UncertainDate)
                    || b.quality_flags.contains(&QualityFlag::AmbiguousLayout)
            }) || id == "yearless"
            {
                ambiguous_guesses += actual
                    .iter()
                    .filter(|e| e.time_precision != Precision::UnknownDate)
                    .count();
            }
            rows.push(serde_json::json!({"id":id,"status":format!("{:?}",r.status),"actual_text":output,"characters":metric(chars[0],chars[1],chars[2]),"semantic_events":metric(sem[0],sem[1],sem[2]),"calendar_events":metric(cal[0],cal[1],cal[2]),"candidates":batch.candidates,"calendar":actual}));
        }
        assert_eq!(ambiguous_guesses, 0, "{fmt}");
        assert!(semantic[0] > 0);
        assert!(calendar[0] > 0);
        formats.insert(fmt.into(),serde_json::json!({"fixtures":25,"characters":metric(char_counts[0],char_counts[1],char_counts[2]),"semantic_events_title":metric(semantic[0],semantic[1],semantic[2]),"calendar_title_precision_date":metric(calendar[0],calendar[1],calendar[2]),"statuses":statuses,"ambiguous_date_guesses":ambiguous_guesses,"cases":rows}));
        drop(service);
        drop(store);
        drop(db);
        std::fs::remove_file(&path).unwrap();
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }
    let report = serde_json::json!({"synthetic_only":true,"ocr_mode":"no engine in 50-case corpus; separately controlled-observation port test","native_product_host":"Unsupported; Windows containment gate open","definitions":{"characters":"Unicode LCS; whole-source gold includes omitted/unsupported positive content","semantic":"one-to-one event title matches, regardless of date precision","calendar":"actual durable events: one-to-one title + precision + local_date; wrong tuples count FP and FN","aggregation":"separate DOCX/XLSX; no excluded unsupported cases"},"formats":formats});
    // Each synthetic run produces new evidence; never replace a historical report.
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.superpowers/sdd/shixu-v0.1");
    std::fs::create_dir_all(&directory).unwrap();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = directory.join(format!(
        "N5-quality-run-{stamp}-{}.json",
        std::process::id()
    ));
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    output
        .write_all(&serde_json::to_vec_pretty(&report).unwrap())
        .unwrap();
    output.sync_all().unwrap();
    println!(
        "N5 per-format metrics {}",
        serde_json::json!({"docx":report["formats"]["docx"].as_object().unwrap().iter().filter(|(k,_)|k.as_str()!="cases").map(|(k,v)|(k.clone(),v.clone())).collect::<serde_json::Map<_,_>>(),"xlsx":report["formats"]["xlsx"].as_object().unwrap().iter().filter(|(k,_)|k.as_str()!="cases").map(|(k,v)|(k.clone(),v.clone())).collect::<serde_json::Map<_,_>>() })
    );
}
#[test]
fn floating_tables_and_duplicate_cell_containers_do_not_claim_complete() {
    let floating = rewrite(
        &fixture("docx", "table"),
        |n, v| {
            if n == "word/document.xml" {
                String::from_utf8(v)
                    .unwrap()
                    .replace(
                        "<w:tbl>",
                        "<w:tbl><w:tblPr><w:tblpPr w:vertAnchor=\"page\"/></w:tblPr>",
                    )
                    .into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    let r = parse("docx", &floating, &ParserLimits::default());
    assert_eq!(r.status, PartStatus::PartialParse);
    assert!(r.blocks.is_empty());
}
#[test]
fn duplicate_worksheet_data_containers_fail_closed() {
    let duplicate = rewrite(
        &fixture("xlsx", "visible"),
        |n, v| {
            if n.ends_with("sheet1.xml") {
                String::from_utf8(v).unwrap().replace("</worksheet>","<sheetData><row r=\"2001\" hidden=\"1\"><c r=\"A2001\" t=\"inlineStr\"><is><t>hidden</t></is></c></row></sheetData></worksheet>").into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    let r = parse("xlsx", &duplicate, &ParserLimits::default());
    assert_eq!(r.status, PartStatus::Unsupported);
    assert!(r.blocks.is_empty());
}
#[test]
fn duplicate_cell_values_fail_closed() {
    let duplicate_value = rewrite(
        &fixture("xlsx", "date1900"),
        |n, v| {
            if n.ends_with("sheet1.xml") {
                String::from_utf8(v)
                    .unwrap()
                    .replace("</c>", "<v>1</v></c>")
                    .into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    assert_eq!(
        parse("xlsx", &duplicate_value, &ParserLimits::default()).status,
        PartStatus::Unsupported
    );
}
#[test]
fn namespace_copy_memory_is_bounded_before_format_classification() {
    let namespace = "x".repeat(10000);
    let source = fixture("docx", "paragraph");
    let changed = rewrite(
        &source,
        |n, v| {
            if n == "word/document.xml" {
                format!("<document xmlns=\"{namespace}\"><body><p>test</p></body></document>")
                    .into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    let limits = ParserLimits {
        max_subprocess_memory_bytes: 40000,
        ..ParserLimits::default()
    };
    assert_eq!(
        parse("docx", &changed, &limits).status,
        PartStatus::LimitExceeded
    );
}
#[test]
fn actual_cumulative_archive_bytes_at_100_mib_and_one_over() {
    let source = fixture("docx", "paragraph");
    let archive = zip::ZipArchive::new(Cursor::new(&source)).unwrap();
    let used = archive.decompressed_size().unwrap() as usize;
    let total = 100 * 1024 * 1024 - used;
    let half = total.div_ceil(2);
    let mut padding = Vec::with_capacity(half);
    let mut seed = 0x47a31b19u32;
    // About 10:1 rather than a ZIP bomb: fresh pseudorandom blocks repeat ten
    // times, so this exercises cumulative output with a <20MiB physical file.
    while padding.len() < half {
        let mut block = [0u8; 16384];
        for b in &mut block {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *b = seed as u8;
        }
        for _ in 0..10 {
            let take = (half - padding.len()).min(block.len());
            padding.extend_from_slice(&block[..take]);
        }
    }
    let boundary = rewrite(
        &source,
        |_, v| v,
        &[
            ("inert-a.dat", &padding),
            ("inert-b.dat", &padding[..total - half]),
        ],
        zip::CompressionMethod::Deflated,
    );
    assert!(boundary.len() < 20 * 1024 * 1024);
    let raised = ParserLimits {
        max_uncompressed_bytes: u64::MAX,
        ..ParserLimits::default()
    };
    let r = parse("docx", &boundary, &raised);
    assert_eq!(r.status, PartStatus::Success);
    assert_eq!(text(&r), "2026年10月12日9:00高数考试");
    let over = rewrite(
        &boundary,
        |_, v| v,
        &[("one-over.dat", b"x")],
        zip::CompressionMethod::Deflated,
    );
    let r = parse("docx", &over, &raised);
    assert_eq!(r.status, PartStatus::LimitExceeded);
    assert!(r.blocks.is_empty());
}
#[test]
fn default_cell_style_zero_retains_its_explicit_date_format() {
    let bytes = rewrite(
        &fixture("xlsx", "date1900"),
        |n, v| {
            if n.ends_with("sheet1.xml") {
                String::from_utf8(v)
                    .unwrap()
                    .replace(" s=\"0\"", "")
                    .into_bytes()
            } else {
                v
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    );
    let r = parse("xlsx", &bytes, &ParserLimits::default());
    assert_eq!(r.status, PartStatus::Success);
    assert_eq!(text(&r), "2026-10-12");
}
fn fix1_change(fmt: &str, id: &str, part: &str, from: &str, to: &str) -> Vec<u8> {
    rewrite(
        &fixture(fmt, id),
        |name, bytes| {
            if name == part {
                let s = String::from_utf8(bytes).unwrap();
                assert!(s.contains(from));
                s.replace(from, to).into_bytes()
            } else {
                bytes
            }
        },
        &[],
        zip::CompressionMethod::Stored,
    )
}
#[test]
fn fix1_f1_default_hidden_rows_are_omitted_but_counted() {
    let bytes = fix1_change(
        "xlsx",
        "visible",
        "xl/worksheets/sheet1.xml",
        "<sheetData>",
        "<sheetFormatPr defaultRowHeight=\"15\" zeroHeight=\"1\"/><sheetData>",
    );
    let r = parse("xlsx", &bytes, &ParserLimits::default());
    assert_eq!(r.status, PartStatus::PartialParse);
    assert!(r.blocks.is_empty());
    let limits = ParserLimits {
        max_xlsx_nonempty_cells: 0,
        ..ParserLimits::default()
    };
    assert_eq!(
        parse("xlsx", &bytes, &limits).status,
        PartStatus::LimitExceeded
    );
}
#[test]
fn fix1_f1_explicit_row_visibility_and_default_visible_controls() {
    for zero_height in ["0", "false", "1", "true"] {
        let bytes = fix1_change(
            "xlsx",
            "visible",
            "xl/worksheets/sheet1.xml",
            "<sheetData>",
            &format!(
                "<sheetFormatPr defaultRowHeight=\"15\" zeroHeight=\"{zero_height}\"/><sheetData>"
            ),
        );
        let override_bytes = rewrite(
            &bytes,
            |name, b| {
                if name.ends_with("sheet1.xml") {
                    String::from_utf8(b)
                        .unwrap()
                        .replace("<row r=\"1\" >", "<row r=\"1\" hidden=\"0\">")
                        .into_bytes()
                } else {
                    b
                }
            },
            &[],
            zip::CompressionMethod::Stored,
        );
        let r = parse("xlsx", &override_bytes, &ParserLimits::default());
        assert_eq!(r.status, PartStatus::Success);
        assert_eq!(text(&r), "2026年10月12日9:00高数考试");
    }
}
#[test]
fn fix1_f2_floating_frame_paragraph_is_partial_and_omitted() {
    let bytes = fix1_change(
        "docx",
        "paragraph",
        "word/document.xml",
        "<w:p>",
        "<w:p><w:pPr><w:framePr w:hAnchor=\"page\" w:vAnchor=\"page\" w:x=\"100\" w:y=\"100\"/></w:pPr>",
    );
    let r = parse("docx", &bytes, &ParserLimits::default());
    assert_eq!(r.status, PartStatus::PartialParse);
    assert!(r.blocks.is_empty());
}
#[test]
fn fix1_f2_normal_paragraph_properties_control() {
    let bytes = fix1_change(
        "docx",
        "paragraph",
        "word/document.xml",
        "<w:p>",
        "<w:p><w:pPr><w:jc w:val=\"center\"/></w:pPr>",
    );
    let r = parse("docx", &bytes, &ParserLimits::default());
    assert_eq!(r.status, PartStatus::Success);
    assert_eq!(text(&r), "2026年10月12日9:00高数考试");
}
#[test]
fn fix1_f3_hidden_onoff_values_preserve_explicitly_visible_runs() {
    let mut errors = vec![];
    for property in ["vanish", "webHidden"] {
        for (value, hidden) in [
            ("0", false),
            ("false", false),
            ("off", false),
            ("1", true),
            ("true", true),
            ("on", true),
            ("", true),
        ] {
            let tag = if value.is_empty() {
                format!("<w:{property}/>")
            } else {
                format!("<w:{property} w:val=\"{value}\"/>")
            };
            let bytes = fix1_change(
                "docx",
                "hidden_run",
                "word/document.xml",
                "<w:vanish/>",
                &tag,
            );
            let r = parse("docx", &bytes, &ParserLimits::default());
            let correct = if hidden {
                r.status == PartStatus::PartialParse && r.blocks.is_empty()
            } else {
                r.status == PartStatus::Success && text(&r) == "2026年10月12日9:00高数考试"
            };
            if !correct {
                errors.push(format!("{property}/{value}: {:?} {}", r.status, text(&r)));
            }
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
#[test]
fn fix1_f3_invalid_onoff_values_fail_without_tentative_text() {
    let mut errors = vec![];
    for property in ["vanish", "webHidden"] {
        for value in ["maybe", "TRUE", "2"] {
            let bytes = fix1_change(
                "docx",
                "hidden_run",
                "word/document.xml",
                "<w:vanish/>",
                &format!("<w:{property} w:val=\"{value}\"/>"),
            );
            let r = parse("docx", &bytes, &ParserLimits::default());
            if r.status != PartStatus::Unsupported || !r.blocks.is_empty() {
                errors.push(format!("{property}/{value}: {:?}", r.status));
            }
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
#[test]
fn fix1_f4_invalid_qnames_and_xmlns_components_fail_closed() {
    let mut errors = vec![];
    for to in [
        "<w:p 1bad=\"x\">",
        "<w:p w:1bad=\"x\">",
        "<w:p xmlns:1bad=\"urn:bad\">",
        "<w:p xmlns:bad:name=\"urn:bad\">",
        "<w:p w:bad:name=\"x\">",
        "<w:p xmlns:_=\"urn:ok\" _:1bad=\"x\">",
    ] {
        let bytes = fix1_change("docx", "paragraph", "word/document.xml", "<w:p>", to);
        let r = parse("docx", &bytes, &ParserLimits::default());
        if r.status != PartStatus::Unsupported || !r.blocks.is_empty() {
            errors.push(format!("{to}: {:?}", r.status));
        }
    }
    let bytes = fix1_change("docx", "paragraph", "word/document.xml", "w:t>", "w:1bad>");
    let r = parse("docx", &bytes, &ParserLimits::default());
    if r.status != PartStatus::Unsupported {
        errors.push(format!("element: {:?}", r.status));
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
#[test]
fn fix1_f4_xml_declarations_require_one_legal_prolog_position() {
    let mut errors = vec![];
    for prefix in [
        "<?xml version=\"1.0\"?><?xml version=\"1.0\"?>",
        " <?xml version=\"1.0\"?>",
        "<!--before--><?xml version=\"1.0\"?>",
        "\n<?xml version=\"1.0\"?>",
        "<?xml version=\"1.0\" unknown=\"x\"?>",
        "<?xml version=\"1.0\" standalone=\"maybe\"?>",
        "<?xml version=\"1.0\" standalone=\"yes\" encoding=\"UTF-8\"?>",
        "<?xml version=\"1.0\" version=\"1.0\"?>",
    ] {
        let bytes = fix1_change(
            "docx",
            "paragraph",
            "word/document.xml",
            "<w:document ",
            &format!("{prefix}<w:document "),
        );
        let r = parse("docx", &bytes, &ParserLimits::default());
        if r.status != PartStatus::Unsupported || !r.blocks.is_empty() {
            errors.push(format!("{prefix}: {:?}", r.status));
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
#[test]
fn fix1_f4_valid_unicode_names_prefixes_and_references_control() {
    let mut errors = vec![];
    for prefix in [
        "",
        "<?xml version=\"1.0\"?>",
        "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
    ] {
        let bytes = fix1_change(
            "docx",
            "paragraph",
            "word/document.xml",
            "<w:document ",
            &format!("{prefix}<w:document "),
        );
        let bytes = rewrite(
            &bytes,
            |n, b| {
                if n == "word/document.xml" {
                    String::from_utf8(b)
                        .unwrap()
                        .replace(
                            "<w:p>",
                            "<w:p xmlns:通知=\"urn:valid\" 通知:字段=\"x\" Á=\"x\">",
                        )
                        .replace("2026年", "&#50;026年")
                        .into_bytes()
                } else {
                    b
                }
            },
            &[],
            zip::CompressionMethod::Stored,
        );
        let r = parse("docx", &bytes, &ParserLimits::default());
        if r.status != PartStatus::Success || text(&r) != "2026年10月12日9:00高数考试" {
            errors.push(format!("valid {prefix}: {:?}", r.status));
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    assert_eq!(result("docx", "renamed_prefix").status, PartStatus::Success);
    assert_eq!(result("docx", "dtd").status, PartStatus::Unsupported);
}
fn fix1_style(apply: &str, xfid: &str) -> String {
    format!(
        "<cellStyleXfs count=\"1\"><xf numFmtId=\"0\"/></cellStyleXfs><cellXfs count=\"1\"><xf numFmtId=\"14\" xfId=\"{xfid}\" applyNumberFormat=\"{apply}\"/></cellXfs>"
    )
}
#[test]
fn fix1_f5_disabled_number_format_cannot_establish_date() {
    let mut errors = vec![];
    for value in ["0", "false"] {
        let bytes = fix1_change(
            "xlsx",
            "date1900",
            "xl/styles.xml",
            "<cellXfs><xf numFmtId=\"14\"/></cellXfs>",
            &fix1_style(value, "0"),
        );
        let r = parse("xlsx", &bytes, &ParserLimits::default());
        if r.status != PartStatus::PartialParse
            || text(&r) != "46307"
            || !r.blocks[0]
                .quality_flags
                .contains(&QualityFlag::UncertainDate)
        {
            errors.push(format!("{value}: {:?} {}", r.status, text(&r)));
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
#[test]
fn fix1_f5_enabled_format_and_formula_controls() {
    for value in ["1", "true"] {
        let bytes = fix1_change(
            "xlsx",
            "date1900",
            "xl/styles.xml",
            "<cellXfs><xf numFmtId=\"14\"/></cellXfs>",
            &fix1_style(value, "0"),
        );
        let r = parse("xlsx", &bytes, &ParserLimits::default());
        assert_eq!(r.status, PartStatus::Success);
        assert_eq!(text(&r), "2026-10-12");
        let formula = rewrite(
            &bytes,
            |n, b| {
                if n.ends_with("sheet1.xml") {
                    String::from_utf8(b)
                        .unwrap()
                        .replace("<v>", "<f>TODAY()</f><v>")
                        .into_bytes()
                } else {
                    b
                }
            },
            &[],
            zip::CompressionMethod::Stored,
        );
        let r = parse("xlsx", &formula, &ParserLimits::default());
        assert_eq!(r.status, PartStatus::PartialParse);
        assert!(
            r.blocks[0]
                .quality_flags
                .contains(&QualityFlag::FormulaDerived)
        );
        assert!(
            r.blocks[0]
                .quality_flags
                .contains(&QualityFlag::UncertainDate)
        );
    }
    assert_eq!(result("xlsx", "date1900").status, PartStatus::Success);
}
#[test]
fn fix1_f1_and_f5_invalid_boolean_format_metadata_fail_closed() {
    let mut errors = vec![];
    for value in ["yes", "on", "2"] {
        let row = fix1_change(
            "xlsx",
            "visible",
            "xl/worksheets/sheet1.xml",
            "<sheetData>",
            &format!("<sheetFormatPr zeroHeight=\"{value}\"/><sheetData>"),
        );
        let r = parse("xlsx", &row, &ParserLimits::default());
        if r.status != PartStatus::Unsupported || !r.blocks.is_empty() {
            errors.push(format!("zeroHeight {value}: {:?}", r.status));
        }
        let style = fix1_change(
            "xlsx",
            "date1900",
            "xl/styles.xml",
            "<cellXfs><xf numFmtId=\"14\"/></cellXfs>",
            &fix1_style(value, "0"),
        );
        let r = parse("xlsx", &style, &ParserLimits::default());
        if r.status != PartStatus::Unsupported || !r.blocks.is_empty() {
            errors.push(format!("applyNumberFormat {value}: {:?}", r.status));
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
