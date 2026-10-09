use crate::contracts::notification::{ParserLimits, PartReason};
#[derive(Debug, Clone, Copy)]
pub enum Resource {
    ImageBytes,
    ImagePixels,
    ImageEdge,
    FileBytes,
    MessageParts,
    MessageBytes,
    PdfPages,
    ExtractedChars,
    XlsxSheets,
    XlsxRows,
    XlsxColumns,
    XlsxCells,
    UncompressedBytes,
    ArchiveEntries,
    CompressionRatio,
    ConcurrentParsers,
    MemoryBytes,
    ImageSeconds,
    FileSeconds,
    PendingTasks,
    CacheBytes,
    DownloadRetries,
}
impl ParserLimits {
    pub fn v01() -> Self {
        Self::default()
    }
    /// Caller overrides may tighten a budget, never raise the v0.1 ceiling.
    pub fn check(&self, r: Resource, used: u64) -> Result<(), PartReason> {
        let cap = self.cap(r).min(Self::default().cap(r));
        if used > cap {
            Err(PartReason::LimitExceeded)
        } else {
            Ok(())
        }
    }
    fn cap(&self, r: Resource) -> u64 {
        use Resource::*;
        match r {
            ImageBytes => self.max_image_bytes,
            ImagePixels => self.max_image_pixels,
            ImageEdge => self.max_image_edge as u64,
            FileBytes => self.max_file_bytes,
            MessageParts => self.max_parts_per_message as u64,
            MessageBytes => self.max_message_bytes,
            PdfPages => self.max_pdf_pages as u64,
            ExtractedChars => self.max_extracted_chars as u64,
            XlsxSheets => self.max_xlsx_sheets as u64,
            XlsxRows => self.max_xlsx_rows_per_sheet as u64,
            XlsxColumns => self.max_xlsx_columns_per_sheet as u64,
            XlsxCells => self.max_xlsx_nonempty_cells as u64,
            UncompressedBytes => self.max_uncompressed_bytes,
            ArchiveEntries => self.max_archive_entries as u64,
            CompressionRatio => self.max_compression_ratio as u64,
            ConcurrentParsers => self.max_concurrent_parsers as u64,
            MemoryBytes => self.max_subprocess_memory_bytes,
            ImageSeconds => self.image_timeout_secs as u64,
            FileSeconds => self.file_timeout_secs as u64,
            PendingTasks => self.max_pending_tasks as u64,
            CacheBytes => self.attachment_cache_bytes,
            DownloadRetries => self.max_download_retries as u64,
        }
    }
    pub fn check_image(&self, bytes: u64, width: u32, height: u32) -> Result<(), PartReason> {
        if width == 0 || height == 0 {
            return Err(PartReason::RecognitionFailed);
        }
        self.check(Resource::ImageBytes, bytes)?;
        self.check(Resource::ImageEdge, width.max(height) as u64)?;
        self.check(Resource::ImagePixels, u64::from(width) * u64::from(height))
    }
    pub fn check_message(&self, sizes: &[u64]) -> Result<(), PartReason> {
        self.check(Resource::MessageParts, sizes.len() as u64)?;
        let total = sizes
            .iter()
            .try_fold(0u64, |sum, n| sum.checked_add(*n))
            .ok_or(PartReason::LimitExceeded)?;
        self.check(Resource::MessageBytes, total)
    }
}
/// Streaming guard. Call before opening an entry and after each bounded read;
/// never extract paths to disk or trust ZIP's declared uncompressed size.
pub struct ArchiveBudget {
    limits: ParserLimits,
    names: std::collections::HashSet<String>,
    total: u64,
    entry_actual: u64,
    entry_compressed: u64,
}
impl ArchiveBudget {
    pub fn new(limits: &ParserLimits) -> Self {
        Self {
            limits: limits.clone(),
            names: Default::default(),
            total: 0,
            entry_actual: 0,
            entry_compressed: 0,
        }
    }
    pub fn begin_entry(
        &mut self,
        name: &str,
        compressed: u64,
        link: bool,
    ) -> Result<(), PartReason> {
        if link
            || name.is_empty()
            || name.contains(['\\', ':'])
            || name.chars().any(char::is_control)
            || name
                .split('/')
                .any(|s| s.is_empty() || s == "." || s == "..")
            || self.names.contains(name)
        {
            return Err(PartReason::FormatUnsupported);
        }
        self.limits
            .check(Resource::ArchiveEntries, self.names.len() as u64 + 1)?;
        self.names.insert(name.into());
        self.entry_actual = 0;
        self.entry_compressed = compressed;
        Ok(())
    }
    pub fn consume(&mut self, actual: u64) -> Result<(), PartReason> {
        if self.names.is_empty() {
            return Err(PartReason::RecognitionFailed);
        }
        let total = self
            .total
            .checked_add(actual)
            .ok_or(PartReason::LimitExceeded)?;
        let entry = self
            .entry_actual
            .checked_add(actual)
            .ok_or(PartReason::LimitExceeded)?;
        self.limits.check(Resource::UncompressedBytes, total)?;
        // u128 multiplication avoids overflow and preserves exact 100:1 boundary.
        let ratio = self
            .limits
            .max_compression_ratio
            .min(ParserLimits::v01().max_compression_ratio);
        if u128::from(entry) > u128::from(self.entry_compressed) * u128::from(ratio) {
            return Err(PartReason::LimitExceeded);
        }
        self.total = total;
        self.entry_actual = entry;
        Ok(())
    }
}
