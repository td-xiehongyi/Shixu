//! Restricted DOCX text/table/inline-image extraction for controlled callers.
use super::{
    host::DetectedType,
    image::{self, OcrEngine},
    ooxml::{self, *},
};
use shixu_core::contracts::{AppResult, error::AppError, notification::*};
use std::path::Path;
/// Native host must cross the Windows containment gate before reading any path.
pub fn extract_docx(_: &Path, _: &str, _: &ParserLimits) -> AppResult<PartResult> {
    Err(AppError::Unsupported)
}
pub fn extract_bytes(bytes: &[u8], id: &str, limits: &ParserLimits) -> AppResult<PartResult> {
    controlled(bytes, id, limits, |_| Ok(None))
}
/// Explicit authorized engine port; no Office/native fallback is provided.
pub fn extract_with_engine(
    bytes: &[u8],
    id: &str,
    limits: &ParserLimits,
    engine: &mut impl OcrEngine,
) -> AppResult<PartResult> {
    controlled(bytes, id, limits, |b| {
        image::extract_with_engine(b, id, limits, engine).map(Some)
    })
}
fn controlled(
    bytes: &[u8],
    id: &str,
    limits: &ParserLimits,
    mut recognize: impl FnMut(&[u8]) -> AppResult<Option<PartResult>>,
) -> AppResult<PartResult> {
    let mut output = Output::new(id)?;
    let parsed = (|| -> Parse<_> {
        let mut package = Package::load(bytes, limits, DetectedType::Docx)?;
        let relationships = package.relationships("word/document.xml", limits)?;
        let root = package.xml("word/document.xml", limits)?;
        if !root.is(W, "document") || root.children.len() != 1 || !root.children[0].is(W, "body") {
            return Err(PartReason::FormatUnsupported);
        }
        let mut state = Document {
            output: &mut output,
            paragraph: 0,
            table: 0,
            images: vec![],
            relationships: &relationships,
            ambiguous: false,
        };
        for n in &root.children[0].children {
            state.block(n, None, limits)?;
        }
        state.output.partial |= package.partial;
        if state.ambiguous {
            for b in &mut state.output.blocks {
                b.quality_flags.push(QualityFlag::AmbiguousLayout)
            }
        }
        // Validate every image reference/type before handing any bytes to the port.
        for (name, _) in &state.images {
            let expected = match package.content_type(name) {
                Some("image/png") => ::image::ImageFormat::Png,
                Some("image/jpeg") => ::image::ImageFormat::Jpeg,
                _ => return Err(PartReason::FormatUnsupported),
            };
            let bytes = package.bytes(name)?;
            limits.check(
                shixu_core::notifications::limits::Resource::ImageBytes,
                bytes.len() as u64,
            )?;
            let actual = ::image::guess_format(bytes).map_err(|_| PartReason::RecognitionFailed)?;
            if actual != expected {
                return Err(PartReason::FormatUnsupported);
            }
            let (width, height) =
                ::image::ImageReader::with_format(std::io::Cursor::new(bytes), actual)
                    .into_dimensions()
                    .map_err(|_| PartReason::RecognitionFailed)?;
            limits.check_image(bytes.len() as u64, width, height)?;
        }
        Ok((package, state.images))
    })();
    let (package, images) = match parsed {
        Ok(v) => v,
        Err(e) => return ooxml::failed(id, e),
    };
    for (name, paragraph) in images {
        let bytes = package.bytes(&name).map_err(|_| AppError::ParseFailed)?;
        match recognize(bytes)? {
            Some(r) if matches!(r.status, PartStatus::Success | PartStatus::PartialParse) => {
                output.partial |= r.status == PartStatus::PartialParse;
                for b in r.blocks {
                    let version = b.engine_version.clone();
                    if let Err(e) = output.push(
                        b.text,
                        PageOrSheet::Paragraph { number: paragraph },
                        b.cell_range_or_bbox,
                        b.method,
                        b.quality_flags,
                        limits,
                    ) {
                        return ooxml::failed(id, e);
                    }
                    if let Some(last) = output.blocks.last_mut() {
                        last.engine_version = version;
                    }
                }
            }
            Some(r) if r.status == PartStatus::LimitExceeded => {
                return ooxml::failed(id, PartReason::LimitExceeded);
            }
            _ => output.partial = true,
        }
    }
    Ok(output.finish())
}
struct Document<'a> {
    output: &'a mut Output,
    paragraph: u32,
    table: u32,
    images: Vec<(String, u32)>,
    relationships: &'a [Relationship],
    ambiguous: bool,
}
impl Document<'_> {
    fn block(&mut self, n: &Node, location: Option<String>, limits: &ParserLimits) -> Parse<()> {
        if n.is(W, "p") {
            self.paragraph = self
                .paragraph
                .checked_add(1)
                .ok_or(PartReason::LimitExceeded)?;
            let mut text = String::new();
            self.run(n, &mut text)?;
            self.output.push(
                text,
                PageOrSheet::Paragraph {
                    number: self.paragraph,
                },
                location.map(|range| EvidenceLocation::CellRange { range }),
                Method::NativeText,
                vec![],
                limits,
            )?;
        } else if n.is(W, "tbl") {
            if location.is_some()
                || n.child(W, "tblPr")
                    .is_some_and(|p| p.child(W, "tblpPr").is_some())
            {
                self.output.partial = true;
                return Ok(());
            }
            self.table += 1;
            let table = self.table;
            for child in &n.children {
                if !child.is(W, "tr") && !child.is(W, "tblPr") && !child.is(W, "tblGrid") {
                    self.output.partial = true
                }
            }

            for (row_index, row) in n.children(W, "tr").enumerate() {
                for child in &row.children {
                    if !child.is(W, "tc") && !child.is(W, "trPr") {
                        self.output.partial = true
                    }
                }
                for (column, cell) in row.children(W, "tc").enumerate() {
                    if column >= 50 || row_index >= 2000 {
                        return Err(PartReason::LimitExceeded);
                    }
                    let coordinate = format!(
                        "table{table}!{}{}",
                        column_name(column as u32 + 1),
                        row_index + 1
                    );
                    for c in &cell.children {
                        if c.is(W, "tcPr") {
                            if c.child(W, "gridSpan").is_some() || c.child(W, "vMerge").is_some() {
                                self.output.partial = true;
                                self.ambiguous = true;
                            }
                        } else {
                            self.block(c, Some(coordinate.clone()), limits)?
                        }
                    }
                }
            }
        } else if n.is(W, "sectPr") {
            self.section(n);
        } else {
            self.output.partial = true
        }
        Ok(())
    }
    fn section(&mut self, n: &Node) {
        for c in &n.children {
            if c.is(W, "cols") && c.attr(W, "num").is_some_and(|v| v != "1") {
                self.output.partial = true;
                self.ambiguous = true;
            }
        }
    }
    fn run(&mut self, n: &Node, text: &mut String) -> Parse<()> {
        if n.is(W, "r")
            && n.child(W, "rPr").is_some_and(|p| {
                p.child(W, "vanish").is_some() || p.child(W, "webHidden").is_some()
            })
        {
            self.output.partial = true;
            return Ok(());
        }
        if n.is(W, "t") {
            text.push_str(&n.text);
            return Ok(());
        }
        if n.is(W, "tab") {
            text.push('\t');
            return Ok(());
        }
        if n.is(W, "br") || n.is(W, "cr") {
            text.push('\n');
            return Ok(());
        }
        if n.is(W, "rPr") {
            return Ok(());
        }
        if n.is(W, "pPr") {
            for c in &n.children {
                if c.is(W, "sectPr") {
                    self.section(c)
                }
            }
            return Ok(());
        }
        if n.is(W, "drawing") {
            let inline: Vec<_> = n.children(WP, "inline").collect();
            if inline.len() != 1 || n.children.iter().any(|c| !c.is(WP, "inline")) {
                self.output.partial = true;
                return Ok(());
            }
            let mut blips = vec![];
            find_blips(inline[0], &mut blips);
            if blips.len() != 1 {
                self.output.partial = true;
                return Ok(());
            }
            if blips[0].attr(R, "link").is_some() {
                self.output.partial = true;
                return Ok(());
            }
            let Some(id) = blips[0].attr(R, "embed") else {
                self.output.partial = true;
                return Ok(());
            };
            let relation = self
                .relationships
                .iter()
                .find(|r| r.id == id && r.kind == "image")
                .ok_or(PartReason::FormatUnsupported)?;
            if let Some(target) = &relation.target {
                self.images.push((target.clone(), self.paragraph));
            } else {
                self.output.partial = true
            }
            return Ok(());
        }
        if n.is(W, "p") || n.is(W, "r") || n.is(W, "hyperlink") {
            if n.is(W, "hyperlink") {
                self.output.partial = true
            }
            for c in &n.children {
                self.run(c, text)?
            }
        } else {
            self.output.partial = true
        }
        Ok(())
    }
}
fn find_blips<'a>(n: &'a Node, out: &mut Vec<&'a Node>) {
    if n.is(A, "blip") {
        out.push(n)
    }
    for c in &n.children {
        find_blips(c, out)
    }
}
pub(crate) fn column_name(mut n: u32) -> String {
    let mut name = String::new();
    while n > 0 {
        n -= 1;
        name.insert(0, char::from(b'A' + (n % 26) as u8));
        n /= 26;
    }
    name
}
