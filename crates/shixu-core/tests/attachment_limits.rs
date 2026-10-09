use shixu_core::{
    contracts::notification::*,
    notifications::{limits::Resource, parts::summarize_parts},
};
use uuid::Uuid;
#[test]
fn each_limit_at_and_one_over() {
    let l = ParserLimits::v01();
    use Resource::*;
    let cases = [
        (ImageBytes, 10 * 1024 * 1024),
        (ImagePixels, 20_000_000),
        (ImageEdge, 10_000),
        (FileBytes, 20 * 1024 * 1024),
        (MessageParts, 5),
        (MessageBytes, 50 * 1024 * 1024),
        (PdfPages, 20),
        (ExtractedChars, 200_000),
        (XlsxSheets, 10),
        (XlsxRows, 2000),
        (XlsxColumns, 50),
        (XlsxCells, 20_000),
        (UncompressedBytes, 100 * 1024 * 1024),
        (ArchiveEntries, 5000),
        (CompressionRatio, 100),
        (ConcurrentParsers, 1),
        (MemoryBytes, 512 * 1024 * 1024),
        (ImageSeconds, 30),
        (FileSeconds, 120),
        (PendingTasks, 100),
        (CacheBytes, 1024 * 1024 * 1024),
        (DownloadRetries, 3),
    ];
    assert_eq!(l.max_pdf_pages, 20);
    for (resource, max) in cases {
        assert_eq!(l.check(resource, max), Ok(()), "{resource:?}");
        assert_eq!(
            l.check(resource, max + 1),
            Err(PartReason::LimitExceeded),
            "{resource:?}"
        );
    }
    assert_eq!(l.download_retry_delays_secs, [60, 300, 1800]);
    assert_eq!(l.non_event_retention_days, 30);
}
#[test]
fn image_dimensions_and_message_totals_are_checked_without_overflow() {
    let l = ParserLimits::v01();
    assert!(l.check_image(10 * 1024 * 1024, 4000, 5000).is_ok());
    assert_eq!(l.check_image(1, 4001, 5000), Err(PartReason::LimitExceeded));
    assert_eq!(
        l.check_image(1, u32::MAX, u32::MAX),
        Err(PartReason::LimitExceeded)
    );
    assert_eq!(l.check_image(1, 0, 10), Err(PartReason::RecognitionFailed));
    assert!(l.check_message(&[10 * 1024 * 1024; 5]).is_ok());
    assert_eq!(l.check_message(&[0; 6]), Err(PartReason::LimitExceeded));
    assert_eq!(
        l.check_message(&[u64::MAX, 1]),
        Err(PartReason::LimitExceeded)
    );
}
fn part(status: PartStatus) -> PartResult {
    PartResult {
        part_id: PartId::from_uuid(Uuid::new_v4()),
        status,
        blocks: vec![],
        reason_code: None,
    }
}
#[test]
fn partial_is_not_non_event() {
    assert_eq!(
        summarize_parts(&[part(PartStatus::Success), part(PartStatus::Unsupported)]),
        PartStatus::PartialParse
    );
    assert_eq!(
        summarize_parts(&[part(PartStatus::Success), part(PartStatus::DownloadFailed)]),
        PartStatus::PartialParse
    );
    assert_eq!(
        summarize_parts(&[part(PartStatus::LimitExceeded)]),
        PartStatus::LimitExceeded
    );
    assert_eq!(
        summarize_parts(&[part(PartStatus::PendingDownload)]),
        PartStatus::PendingDownload
    );
    assert_eq!(summarize_parts(&[]), PartStatus::Success);
}
#[test]
fn zip_bomb_and_traversal_blocked() {
    use shixu_core::notifications::limits::ArchiveBudget;
    let l = ParserLimits::v01();
    for path in [
        "../x", "/x", "C:/x", "a\\..\\x", "a/../x", "a//x", "./x", "a\0b",
    ] {
        assert!(
            ArchiveBudget::new(&l).begin_entry(path, 1, false).is_err(),
            "{path:?}"
        );
    }
    assert!(ArchiveBudget::new(&l).begin_entry("link", 1, true).is_err());
    let mut b = ArchiveBudget::new(&l);
    b.begin_entry("word/document.xml", 10, false).unwrap();
    assert!(b.consume(1000).is_ok());
    assert_eq!(b.consume(1), Err(PartReason::LimitExceeded));
    assert!(b.begin_entry("word/document.xml", 10, false).is_err());
}
