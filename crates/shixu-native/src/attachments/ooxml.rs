//! Controlled OOXML algorithms only. Buffer archive callbacks until full success.
//! No extraction to disk, entity resolver, process launch or network capability.
use super::{container::read_bounded_archive, host::DetectedType};
use quick_xml::{
    events::{BytesDecl, BytesStart, Event},
    name::ResolveResult,
    reader::NsReader,
};
use shixu_core::{
    contracts::{AppResult, error::AppError, notification::*},
    notifications::limits::Resource,
};
use std::collections::BTreeMap;
pub(crate) const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub(crate) const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
pub(crate) const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(crate) const WP: &str =
    "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
pub(crate) const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const P: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
pub(crate) const DOC: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
pub(crate) const BOOK: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
pub(crate) const SHEET: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
pub(crate) const STRINGS: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";
pub(crate) const STYLES: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
pub(crate) type Parse<T> = Result<T, PartReason>;
#[derive(Debug)]
pub(crate) struct Node {
    pub ns: String,
    pub name: String,
    pub attrs: BTreeMap<(String, String), String>,
    pub children: Vec<Node>,
    pub text: String,
}
impl Node {
    pub fn is(&self, ns: &str, name: &str) -> bool {
        self.ns == ns && self.name == name
    }
    pub fn attr(&self, ns: &str, name: &str) -> Option<&str> {
        self.attrs
            .get(&(ns.into(), name.into()))
            .map(String::as_str)
    }
    pub fn child(&self, ns: &str, name: &str) -> Option<&Self> {
        self.children.iter().find(|c| c.is(ns, name))
    }
    pub fn unique_children(&self, ns: &str, names: &[&str]) -> Parse<()> {
        for name in names {
            if self.children.iter().filter(|c| c.is(ns, name)).count() > 1 {
                return Err(PartReason::FormatUnsupported);
            }
        }
        Ok(())
    }
    pub fn children<'a>(&'a self, ns: &'a str, name: &'a str) -> impl Iterator<Item = &'a Self> {
        self.children.iter().filter(move |c| c.is(ns, name))
    }
}
fn namespace(n: ResolveResult<'_>) -> Parse<String> {
    match n {
        ResolveResult::Bound(n) => Ok(n.as_ref().to_owned()),
        ResolveResult::Unbound => Ok(String::new()),
        ResolveResult::Unknown(_) => Err(PartReason::FormatUnsupported),
    }
}
fn valid_text(s: &str) -> bool {
    s.chars()
        .all(|c| matches!(c, '\t' | '\n' | '\r') || c >= ' ' && c != '\u{fffe}' && c != '\u{ffff}')
}
/// UTF-8 profile, strict names/attributes, single root, depth/node/memory budgets.
/// DTD/PI and unknown entities are rejected without any resolver.
fn xml(bytes: &[u8], limits: &ParserLimits, reserved: u64) -> Parse<Node> {
    let input = std::str::from_utf8(bytes).map_err(|_| PartReason::FormatUnsupported)?;
    let mut reader = NsReader::from_str(input);
    reader.config_mut().check_end_names = true;
    let mut stack: Vec<Node> = vec![];
    let mut root = None;
    let mut first_event = true;
    let mut declaration_seen = false;
    let mut nodes = 0u64;
    let mut memory = reserved.saturating_add(bytes.len() as u64);
    loop {
        let event = reader
            .read_event()
            .map_err(|_| PartReason::FormatUnsupported)?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                nodes += 1;
                if nodes > 250_000 || stack.len() >= 64 {
                    return Err(PartReason::LimitExceeded);
                }
                if !qname(e.name().as_ref()) {
                    return Err(PartReason::FormatUnsupported);
                }
                let (ns, local) = reader.resolver().resolve_element(e.name());
                let mut node = Node {
                    ns: namespace(ns)?,
                    name: local.as_ref().into(),
                    attrs: BTreeMap::new(),
                    children: vec![],
                    text: String::new(),
                };
                for (i, a) in e.attributes().enumerate() {
                    if i >= 32 {
                        return Err(PartReason::LimitExceeded);
                    }
                    let a = a.map_err(|_| PartReason::FormatUnsupported)?;
                    if !qname(a.key.as_ref()) {
                        return Err(PartReason::FormatUnsupported);
                    }
                    if a.key.as_ref() == "xmlns" || a.key.as_ref().starts_with("xmlns:") {
                        continue;
                    }
                    let (ns, name) = reader.resolver().resolve_attribute(a.key);
                    let value = a
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|_| PartReason::FormatUnsupported)?
                        .into_owned();
                    if !valid_text(&value) {
                        return Err(PartReason::FormatUnsupported);
                    }
                    memory = memory.saturating_add(value.len() as u64 + 128);
                    if node
                        .attrs
                        .insert((namespace(ns)?, name.as_ref().into()), value)
                        .is_some()
                    {
                        return Err(PartReason::FormatUnsupported);
                    }
                }
                memory = memory.saturating_add(
                    512 + node.ns.len() as u64
                        + node.name.len() as u64
                        + node
                            .attrs
                            .keys()
                            .map(|(ns, n)| ns.len() as u64 + n.len() as u64)
                            .sum::<u64>(),
                );
                limits.check(Resource::MemoryBytes, memory)?;
                if matches!(event, Event::Empty(_)) {
                    append(node, &mut stack, &mut root)?
                } else {
                    stack.push(node)
                }
            }
            Event::End(e) => {
                if !qname(e.name().as_ref()) {
                    return Err(PartReason::FormatUnsupported);
                }
                let node = stack.pop().ok_or(PartReason::FormatUnsupported)?;
                append(node, &mut stack, &mut root)?;
            }
            Event::Text(e) => {
                let s = e.xml_content(quick_xml::XmlVersion::Implicit1_0);
                if !valid_text(&s) {
                    return Err(PartReason::FormatUnsupported);
                }
                add_text(&s, &mut stack)?;
                memory = memory.saturating_add(s.len() as u64);
                limits.check(Resource::MemoryBytes, memory)?;
            }
            Event::CData(e) => {
                let s = e.xml_content(quick_xml::XmlVersion::Implicit1_0);
                if !valid_text(&s) {
                    return Err(PartReason::FormatUnsupported);
                }
                add_text(&s, &mut stack)?;
                memory = memory.saturating_add(s.len() as u64);
                limits.check(Resource::MemoryBytes, memory)?;
            }
            Event::GeneralRef(e) => {
                let s = if let Some(c) = e
                    .resolve_char_ref()
                    .map_err(|_| PartReason::FormatUnsupported)?
                {
                    c.to_string()
                } else {
                    let s = e.as_ref();
                    match s {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => return Err(PartReason::FormatUnsupported),
                    }
                    .into()
                };
                if !valid_text(&s) {
                    return Err(PartReason::FormatUnsupported);
                }
                add_text(&s, &mut stack)?;
            }
            Event::Decl(e) => {
                if !first_event || declaration_seen || root.is_some() || !stack.is_empty() {
                    return Err(PartReason::FormatUnsupported);
                }
                declaration(&e)?;
                declaration_seen = true;
            }
            Event::DocType(_) | Event::PI(_) => return Err(PartReason::FormatUnsupported),
            Event::Comment(_) => {}
            Event::Eof => break,
        }
        first_event = false;
    }
    if !stack.is_empty() {
        return Err(PartReason::FormatUnsupported);
    }
    root.ok_or(PartReason::FormatUnsupported)
}
// XML1.0 Fifth Edition NameStartChar/NameChar, excluding colon for NCName.
// Namespace QNames contain either one NCName or prefix:local NCNames.
fn ncname_start(c: char) -> bool {
    matches!(c, '_'|'A'..='Z'|'a'..='z'|'\u{c0}'..='\u{d6}'|'\u{d8}'..='\u{f6}'|
        '\u{f8}'..='\u{2ff}'|'\u{370}'..='\u{37d}'|'\u{37f}'..='\u{1fff}'|
        '\u{200c}'..='\u{200d}'|'\u{2070}'..='\u{218f}'|'\u{2c00}'..='\u{2fef}'|
        '\u{3001}'..='\u{d7ff}'|'\u{f900}'..='\u{fdcf}'|'\u{fdf0}'..='\u{fffd}'|
        '\u{10000}'..='\u{effff}')
}
fn ncname(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(ncname_start) && chars.all(|c| {
        ncname_start(c)
            || matches!(c,'-'|'.'|'0'..='9'|'\u{b7}'|'\u{300}'..='\u{36f}'|'\u{203f}'..='\u{2040}')
    })
}
fn qname(s: &str) -> bool {
    if let Some((prefix, local)) = s.split_once(':') {
        ncname(prefix) && ncname(local)
    } else {
        ncname(s)
    }
}
fn declaration(e: &BytesDecl<'_>) -> Parse<()> {
    if !valid_text(e.as_ref()) {
        return Err(PartReason::FormatUnsupported);
    }
    let start = BytesStart::from_content(e.as_ref(), 3);
    let mut attrs = start.attributes();
    let version = attrs
        .next()
        .ok_or(PartReason::FormatUnsupported)?
        .map_err(|_| PartReason::FormatUnsupported)?;
    if version.key.as_ref() != "version" || version.value.as_ref() != "1.0" {
        return Err(PartReason::FormatUnsupported);
    }
    let mut stage = 0;
    for attr in attrs {
        let attr = attr.map_err(|_| PartReason::FormatUnsupported)?;
        match attr.key.as_ref() {
            "encoding" if stage == 0 && attr.value.eq_ignore_ascii_case("utf-8") => stage = 1,
            "standalone" if stage < 2 && matches!(attr.value.as_ref(), "yes" | "no") => stage = 2,
            _ => return Err(PartReason::FormatUnsupported),
        }
    }
    Ok(())
}
fn add_text(s: &str, stack: &mut [Node]) -> Parse<()> {
    if let Some(n) = stack.last_mut() {
        n.text.push_str(s)
    } else if !s.trim().is_empty() {
        return Err(PartReason::FormatUnsupported);
    }
    Ok(())
}
fn append(n: Node, stack: &mut [Node], root: &mut Option<Node>) -> Parse<()> {
    if let Some(p) = stack.last_mut() {
        p.children.push(n)
    } else if root.replace(n).is_some() {
        return Err(PartReason::FormatUnsupported);
    }
    Ok(())
}
pub(crate) struct Package {
    parts: BTreeMap<String, zeroize::Zeroizing<Vec<u8>>>,
    defaults: BTreeMap<String, String>,
    overrides: BTreeMap<String, String>,
    pub partial: bool,
}
pub(crate) struct Relationship {
    pub id: String,
    pub kind: String,
    pub target: Option<String>,
}
impl Package {
    pub fn load(bytes: &[u8], limits: &ParserLimits, expected: DetectedType) -> Parse<Self> {
        let mut parts: BTreeMap<String, zeroize::Zeroizing<Vec<u8>>> = BTreeMap::new();
        let mut stored = 0u64;
        let detected = read_bounded_archive(bytes, limits, |n, b| {
            stored = stored.saturating_add(b.len() as u64);
            limits.check(Resource::MemoryBytes, stored.saturating_mul(3))?;
            parts.insert(n.into(), zeroize::Zeroizing::new(b.to_vec()));
            Ok(())
        })?;
        if detected != expected {
            return Err(PartReason::FormatUnsupported);
        }
        // Validate even unvisited XML so an omitted object cannot hide a DTD.
        for (n, b) in &parts {
            if n.ends_with(".xml") || n.ends_with(".rels") {
                xml(b, limits, stored)?;
            }
        }
        let types = xml(
            parts
                .get("[Content_Types].xml")
                .ok_or(PartReason::FormatUnsupported)?,
            limits,
            stored,
        )?;
        if !types.is(CT, "Types") {
            return Err(PartReason::FormatUnsupported);
        }
        let mut p = Self {
            parts,
            defaults: BTreeMap::new(),
            overrides: BTreeMap::new(),
            partial: false,
        };
        for c in &types.children {
            let t = c
                .attr("", "ContentType")
                .ok_or(PartReason::FormatUnsupported)?;
            if t.to_ascii_lowercase().contains("macro")
                || t.to_ascii_lowercase().contains("vba")
                || t.contains("oleObject")
            {
                return Err(PartReason::FormatUnsupported);
            }
            if c.is(CT, "Default") {
                let ext = c
                    .attr("", "Extension")
                    .ok_or(PartReason::FormatUnsupported)?;
                if p.defaults.insert(ext.into(), t.into()).is_some() {
                    return Err(PartReason::FormatUnsupported);
                }
            } else if c.is(CT, "Override") {
                let name = c
                    .attr("", "PartName")
                    .and_then(|v| v.strip_prefix('/'))
                    .ok_or(PartReason::FormatUnsupported)?;
                let name = resolve("", name)?;
                if p.overrides.insert(name, t.into()).is_some() {
                    return Err(PartReason::FormatUnsupported);
                }
            } else {
                return Err(PartReason::FormatUnsupported);
            }
        }
        let root = p.relationships("", limits)?;
        let main = if expected == DetectedType::Docx {
            "word/document.xml"
        } else {
            "xl/workbook.xml"
        };
        let roots: Vec<_> = root.iter().filter(|r| r.kind == "officeDocument").collect();
        if roots.len() != 1 || roots[0].target.as_deref() != Some(main) {
            return Err(PartReason::FormatUnsupported);
        }
        p.require_type(
            main,
            if expected == DetectedType::Docx {
                DOC
            } else {
                BOOK
            },
        )?;
        Ok(p)
    }
    pub fn bytes(&self, name: &str) -> Parse<&[u8]> {
        self.parts
            .get(name)
            .map(|b| b.as_slice())
            .ok_or(PartReason::FormatUnsupported)
    }
    pub fn xml(&self, name: &str, limits: &ParserLimits) -> Parse<Node> {
        xml(
            self.bytes(name)?,
            limits,
            self.parts.values().map(|p| p.len() as u64).sum(),
        )
    }
    pub fn content_type(&self, name: &str) -> Option<&str> {
        self.overrides
            .get(name)
            .or_else(|| {
                name.rsplit_once('.')
                    .and_then(|(_, e)| self.defaults.get(e))
            })
            .map(String::as_str)
    }
    pub fn require_type(&self, name: &str, t: &str) -> Parse<()> {
        if self.content_type(name) == Some(t) {
            Ok(())
        } else {
            Err(PartReason::FormatUnsupported)
        }
    }
    pub fn relationships(
        &mut self,
        source: &str,
        limits: &ParserLimits,
    ) -> Parse<Vec<Relationship>> {
        let name = if source.is_empty() {
            "_rels/.rels".into()
        } else {
            let (dir, file) = source.rsplit_once('/').unwrap_or(("", source));
            format!("{dir}/_rels/{file}.rels")
        };
        let Some(bytes) = self.parts.get(&name) else {
            return Ok(vec![]);
        };
        let root = xml(
            bytes,
            limits,
            self.parts.values().map(|p| p.len() as u64).sum(),
        )?;
        if !root.is(P, "Relationships") {
            return Err(PartReason::FormatUnsupported);
        }
        let mut result = vec![];
        let mut ids = std::collections::BTreeSet::new();
        for c in &root.children {
            if !c.is(P, "Relationship") {
                return Err(PartReason::FormatUnsupported);
            }
            let id = c
                .attr("", "Id")
                .filter(|v| !v.is_empty())
                .ok_or(PartReason::FormatUnsupported)?;
            if !ids.insert(id) {
                return Err(PartReason::FormatUnsupported);
            }
            let kind = c
                .attr("", "Type")
                .and_then(|t| t.strip_prefix(&format!("{R}/")))
                .unwrap_or("unknown");
            let mode = c.attr("", "TargetMode").unwrap_or("Internal");
            let target = c.attr("", "Target").ok_or(PartReason::FormatUnsupported)?;
            if mode == "External" {
                self.partial = true;
                result.push(Relationship {
                    id: id.into(),
                    kind: kind.into(),
                    target: None,
                });
                continue;
            }
            if mode != "Internal" {
                return Err(PartReason::FormatUnsupported);
            }
            if !matches!(
                kind,
                "officeDocument" | "worksheet" | "sharedStrings" | "styles" | "image"
            ) {
                self.partial = true;
                result.push(Relationship {
                    id: id.into(),
                    kind: kind.into(),
                    target: None,
                });
                continue;
            }
            let target = resolve(source, target)?;
            if !self.parts.contains_key(&target) {
                return Err(PartReason::FormatUnsupported);
            }
            result.push(Relationship {
                id: id.into(),
                kind: kind.into(),
                target: Some(target),
            });
        }
        Ok(result)
    }
}
fn resolve(source: &str, target: &str) -> Parse<String> {
    if target.is_empty()
        || target.contains(['\\', ':', '%', '?', '#'])
        || target.chars().any(char::is_control)
    {
        return Err(PartReason::FormatUnsupported);
    }
    let mut path = if target.starts_with('/') {
        vec![]
    } else {
        source
            .rsplit_once('/')
            .map(|(d, _)| d.split('/').collect())
            .unwrap_or_default()
    };
    for s in target.trim_start_matches('/').split('/') {
        match s {
            ".." => {
                path.pop().ok_or(PartReason::FormatUnsupported)?;
            }
            "" | "." => return Err(PartReason::FormatUnsupported),
            _ => path.push(s),
        }
    }
    Ok(path.join("/"))
}
pub(crate) struct Output {
    pub blocks: Vec<EvidenceBlock>,
    pub partial: bool,
    id: PartId,
    chars: u64,
}
impl Output {
    pub fn new(id: &str) -> AppResult<Self> {
        Ok(Self {
            blocks: vec![],
            partial: false,
            id: id.parse().map_err(|_| AppError::InvalidInput)?,
            chars: 0,
        })
    }
    pub fn push(
        &mut self,
        text: String,
        page: PageOrSheet,
        location: Option<EvidenceLocation>,
        method: Method,
        flags: Vec<QualityFlag>,
        limits: &ParserLimits,
    ) -> Parse<()> {
        if text.trim().is_empty() {
            return Ok(());
        }
        self.chars = self.chars.saturating_add(text.chars().count() as u64);
        limits.check(Resource::ExtractedChars, self.chars)?;
        self.blocks.push(EvidenceBlock {
            part_id: self.id,
            page_or_sheet: Some(page),
            cell_range_or_bbox: location,
            text,
            method,
            engine_version: "n5.restricted-ooxml.1".into(),
            quality_flags: flags,
        });
        Ok(())
    }
    pub fn finish(mut self) -> PartResult {
        if self.partial {
            for b in &mut self.blocks {
                if !b.quality_flags.contains(&QualityFlag::PartialSource) {
                    b.quality_flags.push(QualityFlag::PartialSource)
                }
            }
        }
        PartResult {
            part_id: self.id,
            status: if self.partial {
                PartStatus::PartialParse
            } else {
                PartStatus::Success
            },
            blocks: self.blocks,
            reason_code: self.partial.then_some(PartReason::PartialSource),
        }
    }
}
pub(crate) fn failed(id: &str, e: PartReason) -> AppResult<PartResult> {
    super::image::failure(
        id,
        match e {
            PartReason::LimitExceeded => PartStatus::LimitExceeded,
            PartReason::FormatUnsupported => PartStatus::Unsupported,
            _ => PartStatus::RecognitionFailed,
        },
        e,
    )
}
