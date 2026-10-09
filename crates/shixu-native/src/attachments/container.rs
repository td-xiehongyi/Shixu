//! Portable bounded ZIP reader for controlled fixtures and future isolated N5
//! workers. The product host never invokes this outside a validated sandbox.
use super::host::DetectedType;
use shixu_core::{
    contracts::notification::{ParserLimits, PartReason},
    notifications::limits::{ArchiveBudget, Resource},
};
use std::io::{Cursor, Read};
/// Reads every member with actual output accounting and CRC verification. Never
/// writes paths, follows links or executes content. Callback is not a parser
/// permission: production callers must first cross the Windows isolation gate.
/// Callbacks must buffer tentative output and discard it if this returns Err;
/// they must not persist results or perform external actions while reading.
pub fn read_bounded_archive(
    bytes: &[u8],
    limits: &ParserLimits,
    mut member: impl FnMut(&str, &[u8]) -> Result<(), PartReason>,
) -> Result<DetectedType, PartReason> {
    limits.check(Resource::FileBytes, bytes.len() as u64)?;
    if !bytes.starts_with(b"PK\x03\x04") {
        return Err(PartReason::FormatUnsupported);
    }
    // zip's name index collapses duplicate central entries. Check the original
    // EOCD count before constructing it; ZIP64/multidisk/self-extracting layouts
    // are outside this deliberately small OOXML container profile.
    let end = bytes
        .windows(4)
        .rposition(|w| w == b"PK\x05\x06")
        .ok_or(PartReason::FormatUnsupported)?;
    let tail = bytes
        .get(end..)
        .filter(|t| t.len() >= 22)
        .ok_or(PartReason::FormatUnsupported)?;
    let u16_at = |n| u16::from_le_bytes([tail[n], tail[n + 1]]) as usize;
    let u32_at = |n| u32::from_le_bytes([tail[n], tail[n + 1], tail[n + 2], tail[n + 3]]) as usize;
    let count = u16_at(10);
    if u16_at(4) != 0
        || u16_at(6) != 0
        || u16_at(8) != count
        || count == 65535
        || tail.len() != 22 + u16_at(20)
        || u32_at(16).checked_add(u32_at(12)) != Some(end)
    {
        return Err(PartReason::FormatUnsupported);
    }
    limits.check(Resource::ArchiveEntries, count as u64)?;
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| PartReason::RecognitionFailed)?;
    if archive.len() != count || archive.offset() != 0 {
        return Err(PartReason::FormatUnsupported);
    }
    limits.check(Resource::ArchiveEntries, archive.len() as u64)?;
    let mut budget = ArchiveBudget::new(limits);
    let (mut types, mut word, mut sheet) = (false, false, false);
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|_| PartReason::FormatUnsupported)?;
        if entry.encrypted()
            || !matches!(
                entry.compression(),
                zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
            )
        {
            return Err(PartReason::FormatUnsupported);
        }
        let name = entry
            .name()
            .map_err(|_| PartReason::FormatUnsupported)?
            .into_owned();
        // Explicit directories are unnecessary for OOXML and rejected conservatively.
        let special = entry.unix_mode().is_some_and(|m| {
            let kind = m & 0o170000;
            kind != 0 && kind != 0o100000
        });
        budget.begin_entry(&name, entry.compressed_size(), special)?;
        if name.to_ascii_lowercase().ends_with(".bin") {
            return Err(PartReason::FormatUnsupported);
        }
        let start = usize::try_from(entry.data_start().ok_or(PartReason::RecognitionFailed)?)
            .map_err(|_| PartReason::LimitExceeded)?;
        let compressed =
            usize::try_from(entry.compressed_size()).map_err(|_| PartReason::LimitExceeded)?;
        let finish = start
            .checked_add(compressed)
            .filter(|end| *end <= u32_at(16))
            .ok_or(PartReason::RecognitionFailed)?;
        let payload = bytes
            .get(start..finish)
            .ok_or(PartReason::RecognitionFailed)?;
        let mut reader = match entry.compression() {
            zip::CompressionMethod::Stored => Payload::Stored(Cursor::new(payload)),
            zip::CompressionMethod::Deflated => {
                Payload::Deflated(flate2::bufread::DeflateDecoder::new(payload))
            }
            _ => return Err(PartReason::FormatUnsupported),
        };
        let mut crc = crc32fast::Hasher::new();
        let mut content = zeroize::Zeroizing::new(Vec::new());
        let mut buffer = zeroize::Zeroizing::new([0u8; 8192]);
        loop {
            let n = reader
                .read(&mut *buffer)
                .map_err(|_| PartReason::RecognitionFailed)?;
            if n == 0 {
                break;
            }
            budget.consume(n as u64)?;
            crc.update(&buffer[..n]);
            content.extend_from_slice(&buffer[..n]);
        }
        // The decoder's actual consumption must match the bounded ZIP range.
        // Otherwise padding could inflate the ratio denominator. CRC and size
        // are verified ourselves because the ZIP entry reader is not consumed.
        if reader.compressed_read() != entry.compressed_size()
            || crc.finalize() != entry.crc32()
            || content.len() as u64 != entry.size()
        {
            return Err(PartReason::RecognitionFailed);
        }
        types |= name == "[Content_Types].xml";
        word |= name == "word/document.xml";
        sheet |= name == "xl/workbook.xml";
        member(&name, &content)?;
    }
    match (types, word, sheet) {
        (true, true, false) => Ok(DetectedType::Docx),
        (true, false, true) => Ok(DetectedType::Xlsx),
        _ => Err(PartReason::FormatUnsupported),
    }
}
/// Structural ZIP classification, not OOXML semantic validation or extraction.
pub fn inspect_container(bytes: &[u8], limits: &ParserLimits) -> Result<DetectedType, PartReason> {
    read_bounded_archive(bytes, limits, |_, _| Ok(()))
}

enum Payload<'a> {
    Stored(Cursor<&'a [u8]>),
    Deflated(flate2::bufread::DeflateDecoder<&'a [u8]>),
}
impl Payload<'_> {
    fn compressed_read(&self) -> u64 {
        match self {
            Self::Stored(r) => r.position(),
            Self::Deflated(r) => r.total_in(),
        }
    }
}
impl Read for Payload<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Stored(r) => r.read(out),
            Self::Deflated(r) => r.read(out),
        }
    }
}
