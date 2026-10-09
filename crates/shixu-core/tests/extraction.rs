use shixu_core::{
    contracts::{calendar::Precision, error::AppError, notification::*},
    notifications::{aggregate::merge_parts, extract::extract, time::parse_time},
};
use uuid::Uuid;
const SENT: i64 = 1_791_532_800_000; // 2026-10-09T08:00:00Z
fn envelope(text: &str) -> MessageEnvelope {
    MessageEnvelope {
        message_key: MessageKey::from_uuid(Uuid::from_u128(1)),
        source_id: SourceId::from_uuid(Uuid::from_u128(2)),
        account_id: "synthetic".into(),
        group_id: "g".into(),
        native_message_id: "1".into(),
        sent_at: SENT,
        received_at: SENT + 1000,
        sender_id: "sender".into(),
        text: text.into(),
        reply_to: None,
        revision: 1,
        revoked: false,
        processing_state: ProcessingState::Persisted,
        parts: vec![],
    }
}
fn batch(text: &str) -> ExtractBatch {
    extract(&envelope(text), &[], &[], "Asia/Shanghai").unwrap()
}
fn block(text: &str) -> EvidenceBlock {
    EvidenceBlock {
        part_id: PartId::from_uuid(Uuid::from_u128(3)),
        page_or_sheet: Some(PageOrSheet::Page { number: 1 }),
        cell_range_or_bbox: Some(EvidenceLocation::BoundingBox {
            x: 0,
            y: 0,
            width: 200,
            height: 30,
        }),
        text: text.into(),
        method: Method::Ocr,
        engine_version: "synthetic-layout-only".into(),
        quality_flags: vec![],
    }
}
#[test]
fn design_time_examples() {
    let exact = parse_time("2026年10月12日 09:00—11:00 高数考试", SENT, "Asia/Shanghai").unwrap();
    assert_eq!(exact.precision, Precision::Exact);
    assert_eq!(exact.start_at, Some(1_791_766_800_000));
    assert_eq!(exact.end_at, Some(1_791_774_000_000));
    let tomorrow = parse_time("明天 9:00 高数考试", SENT, "Asia/Shanghai").unwrap();
    assert_eq!(tomorrow.local_date.as_deref(), Some("2026-10-10"));
    assert_eq!(tomorrow.end_at, None);
    for text in ["2026年10月12日高数考试", "明天上午考试"] {
        assert_eq!(
            parse_time(text, SENT, "Asia/Shanghai").unwrap().precision,
            Precision::DateOnly
        );
    }
    for text in ["下周考试", "近期补考", "周三考试", "2026年10月12日（周二）"] {
        assert_eq!(
            parse_time(text, SENT, "Asia/Shanghai").unwrap().precision,
            Precision::UnknownDate
        );
    }
    assert_eq!(
        parse_time("2026年10月12日全天活动", SENT, "Asia/Shanghai")
            .unwrap()
            .precision,
        Precision::ExplicitAllDay
    );
}
#[test]
fn year_missing_stays_unknown() {
    let yearless = batch("10月12日考试");
    assert_eq!(
        yearless.candidates[0].time.precision,
        Precision::UnknownDate
    );
}
#[test]
fn dst_gap_and_fold_stay_ambiguous() {
    for text in ["2026-03-08 02:30 考试", "2026-11-01 01:30 考试"] {
        let t = parse_time(text, SENT, "America/New_York").unwrap();
        assert_eq!(t.precision, Precision::UnknownDate);
        assert_eq!(t.start_at, None);
    }
}
#[test]
fn negation_question_is_not_event() {
    for t in [
        "明天考试吗？",
        "明天不考试",
        "听说明天考试",
        "可能明天会议",
        "我今天考完试了",
        "考试真难",
        "请问报名截止了吗",
    ] {
        assert!(batch(t).candidates.is_empty(), "{t}");
    }
}
#[test]
fn one_message_two_events() {
    let b = batch("明天9:00高数考试；后天14:00班级会议");
    assert_eq!(b.candidates.len(), 2);
    assert_ne!(b.candidates[0].candidate_key, b.candidates[1].candidate_key);
}
#[test]
fn image_text_date_conflict_stays_unknown() {
    let b = extract(
        &envelope("2026年10月12日高数考试"),
        &[block("2026年10月13日高数考试")],
        &[],
        "Asia/Shanghai",
    )
    .unwrap();
    assert_eq!(b.candidates.len(), 1);
    assert_eq!(b.candidates[0].time.precision, Precision::UnknownDate);
    assert_eq!(b.candidates[0].evidence.len(), 2);
}
#[test]
fn missing_attachment_is_not_no_event() {
    let mut e = envelope("考试安排见附件");
    e.parts.push(MessagePart {
        part_id: PartId::from_uuid(Uuid::from_u128(3)),
        message_key: e.message_key,
        kind: PartKind::Image,
        source_file_ref: None,
        original_name: None,
        declared_type: None,
        detected_type: None,
        byte_size: None,
        content_hash: None,
        fetch_state: FetchState::Unavailable,
        parse_state: PartStatus::DownloadFailed,
        failure_code: Some(PartReason::DownloadUnavailable),
        encrypted_blob_ref: None,
        retained_until: None,
    });
    let b = extract(&e, &[], &[], "Asia/Shanghai").unwrap();
    assert_eq!(b.part_results[0].status, PartStatus::DownloadFailed);
    assert_eq!(b.candidates[0].time.precision, Precision::UnknownDate);
}
#[test]
fn late_attachment_does_not_revert_newer_notice() {
    let original = batch("明天高数考试");
    let original_order = original.source_order;
    let result = PartResult {
        part_id: PartId::from_uuid(Uuid::from_u128(3)),
        status: PartStatus::Success,
        blocks: vec![block("后天高数考试")],
        reason_code: None,
    };
    let late_batch = merge_parts(&original, &[result]).unwrap();
    assert_eq!(late_batch.source_order, original_order);
    assert_eq!(late_batch.message_revision, original.message_revision);
    assert_eq!(
        late_batch.candidates[0].candidate_key,
        original.candidates[0].candidate_key
    );
    assert_eq!(
        late_batch.candidates[0].time.precision,
        Precision::UnknownDate
    );
    assert_eq!(merge_parts(&late_batch, &[]).unwrap(), late_batch);
}
#[test]
fn identity_survives_date_edit_and_delivery_time() {
    let old = batch("明天9:00高数考试");
    let mut edited = envelope("后天10:00高数考试");
    edited.revision = 2;
    edited.received_at += 999999;
    let new = extract(&edited, &[], &[], "Asia/Shanghai").unwrap();
    assert_eq!(
        old.candidates[0].candidate_key,
        new.candidates[0].candidate_key
    );
    assert_eq!(old.source_order, new.source_order);
    assert_eq!(new.message_revision, 2);
}
#[test]
fn source_order_equal_timestamps_are_not_fabricated_sequence() {
    let a = envelope("明天考试");
    let mut b = a.clone();
    b.message_key = MessageKey::from_uuid(Uuid::from_u128(11));
    b.received_at += 2000;
    assert_eq!(
        extract(&a, &[], &[], "Asia/Shanghai").unwrap().source_order,
        extract(&b, &[], &[], "Asia/Shanghai").unwrap().source_order
    );
}
#[test]
fn evidence_spans_are_utf8_source_ranges() {
    let e = envelope("通知：明天高数考试；后天会议");
    let b = extract(&e, &[], &[], "Asia/Shanghai").unwrap();
    assert_eq!(b.candidates.len(), 2);
    for c in b.candidates {
        for ev in c.evidence {
            let Some(EvidenceLocation::TextSpan { start, end }) = ev.cell_range_or_bbox else {
                panic!("missing span")
            };
            assert_eq!(
                e.text.get(start as usize..end as usize),
                Some(ev.text.as_str())
            );
            assert!(ev.text.contains(&c.title));
        }
    }
}
#[test]
fn independent_columns_and_rows_never_join() {
    let date = block("2026年10月12日");
    let mut subject = block("高数考试");
    subject.cell_range_or_bbox = Some(EvidenceLocation::BoundingBox {
        x: 400,
        y: 0,
        width: 200,
        height: 30,
    });
    let b = extract(&envelope(""), &[date, subject], &[], "Asia/Shanghai").unwrap();
    assert_eq!(b.candidates.len(), 1);
    assert_eq!(b.candidates[0].time.precision, Precision::UnknownDate);
    let mut date = block("2026年10月12日");
    date.method = Method::Cell;
    date.cell_range_or_bbox = Some(EvidenceLocation::CellRange { range: "A1".into() });
    let mut subject = block("高数考试");
    subject.method = Method::Cell;
    subject.cell_range_or_bbox = Some(EvidenceLocation::CellRange { range: "B2".into() });
    let b = extract(&envelope(""), &[date, subject], &[], "Asia/Shanghai").unwrap();
    assert_eq!(b.candidates[0].time.precision, Precision::UnknownDate);
}
#[test]
fn flagged_dates_require_independent_corroboration() {
    for flag in [
        QualityFlag::AmbiguousLayout,
        QualityFlag::PartialSource,
        QualityFlag::UncertainDate,
        QualityFlag::FormulaDerived,
    ] {
        let mut ev = block("2026年10月12日高数考试");
        ev.quality_flags.push(flag);
        let b = extract(&envelope(""), &[ev.clone()], &[], "Asia/Shanghai").unwrap();
        assert_eq!(b.candidates[0].time.precision, Precision::UnknownDate);
        assert!(b.candidates[0].evidence[0].quality_flags.contains(&flag));
        let corroborated = extract(
            &envelope("2026年10月12日高数考试"),
            &[ev],
            &[],
            "Asia/Shanghai",
        )
        .unwrap();
        assert_eq!(
            corroborated.candidates[0].time.precision,
            Precision::DateOnly
        );
    }
}
#[test]
fn invalid_and_inverted_dates_do_not_guess() {
    for text in [
        "2026年2月30日考试",
        "2026-10-12 11:00—09:00考试",
        "2026-10-12 25:00考试",
        "2026-10-12或2026-10-13考试",
    ] {
        assert_eq!(
            parse_time(text, SENT, "Asia/Shanghai").unwrap().precision,
            Precision::UnknownDate,
            "{text}"
        );
    }
    assert_eq!(
        parse_time("明天考试", SENT, "not/a/zone"),
        Err(AppError::InvalidInput)
    );
}
#[test]
fn relative_arithmetic_uses_local_calendar() {
    for (at, expected) in [
        ("2024-02-28T16:30:00Z", "2024-03-01"),
        ("2026-12-31T08:00:00Z", "2027-01-01"),
        ("2026-04-30T08:00:00Z", "2026-05-01"),
    ] {
        let at = chrono::DateTime::parse_from_rfc3339(at)
            .unwrap()
            .timestamp_millis();
        assert_eq!(
            parse_time("明天考试", at, "Asia/Shanghai")
                .unwrap()
                .local_date
                .as_deref(),
            Some(expected)
        );
    }
    let at = chrono::DateTime::parse_from_rfc3339("2024-02-28T08:00:00Z")
        .unwrap()
        .timestamp_millis();
    assert_eq!(
        parse_time("明天考试", at, "Asia/Shanghai")
            .unwrap()
            .local_date
            .as_deref(),
        Some("2024-02-29")
    );
}
#[test]
fn explicit_reply_only_and_bounded_context() {
    let original = envelope("2026年10月12日高数考试");
    let mut notice = envelope("原定高数考试改至明天9:00");
    notice.message_key = MessageKey::from_uuid(Uuid::from_u128(9));
    notice.reply_to = Some(original.message_key);
    let b = extract(
        &notice,
        &[],
        std::slice::from_ref(&original),
        "Asia/Shanghai",
    )
    .unwrap();
    assert_eq!(b.candidates[0].action, CandidateAction::Reschedule);
    assert_eq!(
        b.candidates[0].target_message_key,
        Some(original.message_key)
    );
    let mut other = original.clone();
    other.group_id = "other".into();
    assert_eq!(
        extract(&notice, &[], &[other], "Asia/Shanghai"),
        Err(AppError::InvalidInput)
    );
    assert_eq!(
        extract(&notice, &[], &vec![original.clone(); 6], "Asia/Shanghai"),
        Err(AppError::InvalidInput)
    );
    let mut huge = original.clone();
    huge.text = "x".repeat(20001);
    assert_eq!(
        extract(&notice, &[], &[huge], "Asia/Shanghai"),
        Err(AppError::InvalidInput)
    );
    notice.reply_to = None;
    assert_eq!(
        extract(&notice, &[], &[original], "Asia/Shanghai")
            .unwrap()
            .candidates[0]
            .target_message_key,
        None
    );
}

#[test]
fn text_quality_dataset_reports_actual_errors() {
    use serde_json::{Value, json};
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/extraction/cases.json"
    ))
    .unwrap();
    let expected: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/extraction/expected.json"
    ))
    .unwrap();
    assert!(cases.len() >= 120);
    assert_eq!(cases.len(), expected.len());
    let mut tp = 0usize;
    let mut fp = 0usize;
    let mut fn_count = 0usize;
    let mut guesses = 0usize;
    let mut unsupported = 0usize;
    let mut actual = vec![];
    for (case, label) in cases.iter().zip(&expected) {
        assert_eq!(case["id"], label["id"]);
        let mut e = envelope(case["text"].as_str().unwrap());
        e.sent_at = case["sent_at"].as_i64().unwrap();
        let mut context = vec![];
        if let Some(reference) = case.get("context") {
            let mut original = envelope(reference["text"].as_str().unwrap());
            original.message_key = reference["message_key"].as_str().unwrap().parse().unwrap();
            original.sent_at = reference["sent_at"].as_i64().unwrap();
            e.reply_to = Some(original.message_key);
            context.push(original);
        }
        let result = extract(&e, &[], &context, case["timezone"].as_str().unwrap()).unwrap();
        let actual_events:Vec<Value>=result.candidates.iter().map(|c|json!({"kind":c.kind,"action":c.action,"precision":c.time.precision,"local_date":c.time.local_date,"start_at":c.time.start_at,"end_at":c.time.end_at,"target":c.target_message_key.is_some()})).collect();
        let mut remaining = label["events"].as_array().unwrap().clone();
        let mut case_tp = 0;
        let mut case_fp = 0;
        let expected_ambiguous: Vec<_> = remaining
            .iter()
            .filter(|v| v["precision"] == "unknown_date")
            .cloned()
            .collect();
        for a in &actual_events {
            if let Some(index) = remaining.iter().position(|v| v == a) {
                remaining.remove(index);
                case_tp += 1;
            } else {
                case_fp += 1;
            }
        }
        guesses += actual_events
            .iter()
            .filter(|a| {
                a["precision"] != "unknown_date"
                    && expected_ambiguous
                        .iter()
                        .any(|e| e["kind"] == a["kind"] && e["action"] == a["action"])
            })
            .count();
        tp += case_tp;
        fp += case_fp;
        fn_count += remaining.len();
        if !label["unsupported"].is_null() {
            unsupported += 1;
        }
        actual.push(json!({"id":case["id"],"text":case["text"],"expected":label["events"],"actual":actual_events,"candidates":result.candidates,"part_results":result.part_results,"tp":case_tp,"fp":case_fp,"fn":remaining.len(),"unsupported":label["unsupported"]}));
    }
    let precision = tp as f64 / (tp + fp) as f64;
    let recall = tp as f64 / (tp + fn_count) as f64;
    let metrics = json!({"cases":cases.len(),"tp":tp,"fp":fp,"fn":fn_count,"precision":precision,"recall":recall,"ambiguous_date_guesses":guesses,"unsupported_cases":unsupported,"results":actual});
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.superpowers/sdd/shixu-v0.1/N6-text-actual.json");
    std::fs::write(path, serde_json::to_string_pretty(&metrics).unwrap() + "\n").unwrap();
    println!(
        "N6 text quality: cases={} TP={tp} FP={fp} FN={fn_count} precision={precision:.4} recall={recall:.4} ambiguous_guesses={guesses} unsupported={unsupported}",
        cases.len()
    );
    assert!(precision >= 0.95, "precision {precision}, TP {tp}, FP {fp}");
    assert!(recall >= 0.90, "recall {recall}, TP {tp}, FN {fn_count}");
    assert_eq!(guesses, 0);
}

#[test]
fn natural_language_questions_are_not_declarations() {
    for text in ["物理考试怎么复习", "会议室怎么走"] {
        assert!(batch(text).candidates.is_empty(), "{text}");
    }
}
#[test]
fn duplicate_subjects_with_independent_times_stay_conflict_capable() {
    assert_eq!(
        extract(
            &envelope("明天高数考试；后天高数考试"),
            &[],
            &[],
            "Asia/Shanghai"
        ),
        Err(AppError::Conflict)
    );
}
#[test]
fn explicit_reply_can_supply_year_but_never_unrelated_context() {
    let original = envelope("2026年10月12日高数考试");
    let mut notice = envelope("原定高数考试改至10月14日9:00");
    notice.message_key = MessageKey::from_uuid(Uuid::from_u128(19));
    notice.reply_to = Some(original.message_key);
    let b = extract(
        &notice,
        &[],
        std::slice::from_ref(&original),
        "Asia/Shanghai",
    )
    .unwrap();
    assert_eq!(b.candidates[0].time.precision, Precision::Exact);
    assert_eq!(
        b.candidates[0].time.local_date.as_deref(),
        Some("2026-10-14")
    );
    assert_eq!(b.candidates[0].time.raw_time_text, notice.text);
    assert_eq!(b.candidates[0].evidence.len(), 2);
    assert_ne!(
        b.candidates[0].evidence[0].part_id,
        b.candidates[0].evidence[1].part_id
    );
    notice.reply_to = None;
    assert_eq!(
        extract(&notice, &[], &[original], "Asia/Shanghai")
            .unwrap()
            .candidates[0]
            .time
            .precision,
        Precision::UnknownDate
    );
}
#[test]
fn merge_refuses_missing_or_invalid_provenance() {
    let mut b = batch("");
    b.extractor_version = "n6.rules.1".into();
    assert_eq!(merge_parts(&b, &[]), Err(AppError::InvalidInput));
    b.extractor_version = "n6.rules.1;timezone=not/a/zone".into();
    assert_eq!(merge_parts(&b, &[]), Err(AppError::InvalidInput));
}
#[test]
fn attachment_only_late_parts_use_original_anchor_and_zone() {
    let b = batch("");
    let part = PartResult {
        part_id: PartId::from_uuid(Uuid::from_u128(3)),
        status: PartStatus::Success,
        blocks: vec![block("明天9:00高数考试")],
        reason_code: None,
    };
    let merged = merge_parts(&b, &[part]).unwrap();
    assert_eq!(
        merged.candidates[0].time.local_date.as_deref(),
        Some("2026-10-10")
    );
    assert_eq!(merged.candidates[0].time.timezone, "Asia/Shanghai");
    assert_eq!(merged.source_order, b.source_order);
}
#[test]
fn partial_parse_status_cannot_promote_unflagged_dates() {
    let b = batch("");
    let part = PartResult {
        part_id: PartId::from_uuid(Uuid::from_u128(3)),
        status: PartStatus::PartialParse,
        blocks: vec![block("明天9:00高数考试")],
        reason_code: Some(PartReason::PartialSource),
    };
    let result = merge_parts(&b, &[part]).unwrap();
    assert_eq!(result.candidates[0].time.precision, Precision::UnknownDate);
    assert!(
        result.candidates[0].evidence[0]
            .quality_flags
            .contains(&QualityFlag::PartialSource)
    );
}
#[test]
fn source_revisions_and_identity_do_not_use_edited_time_text() {
    let a = batch("明天高数考试；后天英语补考");
    let b = batch("后天英语补考；2026年10月20日高数考试");
    assert_eq!(a.candidates[0].candidate_key, b.candidates[1].candidate_key);
    assert_eq!(a.candidates[1].candidate_key, b.candidates[0].candidate_key);
}
#[test]
fn same_day_range_never_rolls_over_without_evidence() {
    let explicit = parse_time("2026-10-12 23:00—次日01:00考试", SENT, "Asia/Shanghai").unwrap();
    assert_eq!(explicit.precision, Precision::Exact);
    assert!(explicit.end_at.unwrap() > explicit.start_at.unwrap());
    let inverted = parse_time("2026-10-12 23:00—01:00考试", SENT, "Asia/Shanghai").unwrap();
    assert_eq!(inverted.precision, Precision::UnknownDate);
}
#[test]
fn invalid_envelope_timestamp_and_mismatched_part_fail_closed() {
    let mut e = envelope("明天考试");
    e.sent_at = -1;
    assert_eq!(
        extract(&e, &[], &[], "Asia/Shanghai"),
        Err(AppError::InvalidInput)
    );
    let b = batch("");
    let part = PartResult {
        part_id: PartId::from_uuid(Uuid::from_u128(4)),
        status: PartStatus::Success,
        blocks: vec![block("明天考试")],
        reason_code: None,
    };
    assert_eq!(merge_parts(&b, &[part]), Err(AppError::InvalidInput));
}

#[test]
fn unsupported_positive_text_is_explicitly_partial_not_non_event() {
    for text in [
        "2026年10月12日数学测验",
        "明天研究生答辩",
        "明天考试；后天答辩",
    ] {
        let b = batch(text);
        assert!(
            b.part_results
                .iter()
                .any(|p| p.status == PartStatus::PartialParse
                    && p.reason_code == Some(PartReason::PartialSource)
                    && p.blocks.iter().any(|e| e.text == text)),
            "{text}"
        );
    }
}
#[test]
fn corroboration_compares_time_values_not_wording() {
    let mut ev = block("高数考试2026年10月12日");
    ev.quality_flags.push(QualityFlag::UncertainDate);
    let b = extract(
        &envelope("2026年10月12日高数考试"),
        &[ev],
        &[],
        "Asia/Shanghai",
    )
    .unwrap();
    assert_eq!(b.candidates[0].time.precision, Precision::DateOnly);
}
#[test]
fn explicit_clock_enriches_same_date_without_inventing_conflict() {
    let b = extract(
        &envelope("2026年10月12日高数考试"),
        &[block("2026年10月12日09:00高数考试")],
        &[],
        "Asia/Shanghai",
    )
    .unwrap();
    assert_eq!(b.candidates[0].time.precision, Precision::Exact);
}
#[test]
fn uncertain_clock_cannot_be_promoted_from_date_only_text() {
    let mut ev = block("2026年10月12日09:00高数考试");
    ev.quality_flags.push(QualityFlag::UncertainDate);
    let b = extract(
        &envelope("2026年10月12日高数考试"),
        &[ev],
        &[],
        "Asia/Shanghai",
    )
    .unwrap();
    assert_eq!(b.candidates[0].time.precision, Precision::DateOnly);
    assert_eq!(b.candidates[0].time.start_at, None);
}
#[test]
fn date_conflict_is_flagged_and_cannot_recover_by_replaying_one_side() {
    let original = batch("2026年10月12日高数考试");
    let first = PartResult {
        part_id: PartId::from_uuid(Uuid::from_u128(3)),
        status: PartStatus::Success,
        blocks: vec![block("2026年10月13日高数考试")],
        reason_code: None,
    };
    let conflict = merge_parts(&original, std::slice::from_ref(&first)).unwrap();
    assert!(
        conflict.candidates[0]
            .evidence
            .iter()
            .any(|e| e.quality_flags.contains(&QualityFlag::UncertainDate))
    );
    assert_eq!(
        merge_parts(&conflict, &[first]).unwrap().candidates[0]
            .time
            .precision,
        Precision::UnknownDate
    );
}
#[test]
fn failed_parse_with_blocks_is_rejected() {
    let part = PartResult {
        part_id: PartId::from_uuid(Uuid::from_u128(3)),
        status: PartStatus::RecognitionFailed,
        blocks: vec![block("明天考试")],
        reason_code: Some(PartReason::RecognitionFailed),
    };
    assert_eq!(
        merge_parts(&batch(""), &[part]),
        Err(AppError::InvalidInput)
    );
}
#[test]
fn complete_body_and_partial_attachment_preserve_separate_statuses() {
    let mut e = envelope("明天考试");
    e.parts.push(MessagePart {
        part_id: PartId::from_uuid(Uuid::from_u128(3)),
        message_key: e.message_key,
        kind: PartKind::Image,
        source_file_ref: None,
        original_name: None,
        declared_type: None,
        detected_type: None,
        byte_size: None,
        content_hash: None,
        fetch_state: FetchState::Fetched,
        parse_state: PartStatus::PartialParse,
        failure_code: Some(PartReason::PartialSource),
        encrypted_blob_ref: None,
        retained_until: None,
    });
    let b = extract(&e, &[block("明天会议")], &[], "Asia/Shanghai").unwrap();
    assert_eq!(
        b.candidates
            .iter()
            .find(|c| c.kind == "meeting")
            .unwrap()
            .time
            .precision,
        Precision::UnknownDate
    );
    assert_eq!(b.part_results[0].status, PartStatus::PartialParse);
}

#[test]
fn negated_changes_and_event_materials_are_not_event_declarations() {
    for text in [
        "明天考试不取消",
        "明天会议不会取消",
        "考试资料明天发布",
        "会议纪要明天提交",
        "活动照片后天发布",
        "高数考试成绩今天发布",
    ] {
        assert!(batch(text).candidates.is_empty(), "{text}");
    }
}
#[test]
fn location_requires_explicit_source_field() {
    assert_eq!(batch("高数考试在明天举行").candidates[0].location, None);
    assert_eq!(
        batch("明天高数考试，地点：A301，请携带证件").candidates[0]
            .location
            .as_deref(),
        Some("A301")
    );
}
#[test]
fn body_part_id_routes_virtual_and_explicit_body_evidence() {
    use shixu_core::notifications::extract::body_part_id;
    let mut e = envelope("明天考试");
    let virtual_id = body_part_id(&e);
    let b = extract(&e, &[], &[], "Asia/Shanghai").unwrap();
    assert_eq!(b.candidates[0].evidence[0].part_id, virtual_id);
    assert_eq!(b.part_results.last().unwrap().part_id, virtual_id);
    e.parts.push(MessagePart {
        part_id: PartId::from_uuid(Uuid::from_u128(99)),
        message_key: e.message_key,
        kind: PartKind::Text,
        source_file_ref: None,
        original_name: None,
        declared_type: None,
        detected_type: None,
        byte_size: None,
        content_hash: None,
        fetch_state: FetchState::Fetched,
        parse_state: PartStatus::Success,
        failure_code: None,
        encrypted_blob_ref: None,
        retained_until: None,
    });
    assert_eq!(body_part_id(&e), e.parts[0].part_id);
    assert_ne!(body_part_id(&e), virtual_id);
    e.parts.clear();
    e.message_key = MessageKey::from_uuid(Uuid::from_u128(100));
    assert_ne!(body_part_id(&e), virtual_id);
}

#[test]
fn fuzzy_relative_expressions_do_not_match_substrings_as_definite_days() {
    for text in [
        "大后天9:00考试",
        "明天的明天考试",
        "明天或周三考试",
        "可能明天9:00考试",
    ] {
        assert_eq!(
            parse_time(text, SENT, "Asia/Shanghai").unwrap().precision,
            Precision::UnknownDate,
            "{text}"
        );
    }
}
#[test]
fn even_empty_batches_validate_source_timestamp_range() {
    let mut e = envelope("");
    e.sent_at = i64::MAX;
    assert_eq!(
        extract(&e, &[], &[], "Asia/Shanghai"),
        Err(AppError::InvalidInput)
    );
}

#[test]
fn review_f1_negated_reschedules_cannot_target_replied_event() {
    let original = envelope("2026年10月12日高数考试");
    for text in [
        "高数考试不改到2026年10月13日，仍按原计划",
        "原定高数考试不会延期至明天",
        "高数考试不改至明天",
        "高数考试不会调整到明天",
        "高数考试不延期至明天",
        "高数考试不会调整至明天",
    ] {
        let mut notice = envelope(text);
        notice.message_key = MessageKey::from_uuid(Uuid::from_u128(90));
        notice.reply_to = Some(original.message_key);
        let b = extract(
            &notice,
            &[],
            std::slice::from_ref(&original),
            "Asia/Shanghai",
        )
        .unwrap();
        assert!(b.candidates.is_empty(), "{text}");
        assert_eq!(
            parse_time(text, SENT, "Asia/Shanghai").unwrap().precision,
            Precision::UnknownDate,
            "{text}"
        );
    }
    let mut valid = envelope("高数考试改到明天9:00");
    valid.reply_to = Some(original.message_key);
    let b = extract(&valid, &[], &[original], "Asia/Shanghai").unwrap();
    assert_eq!(b.candidates[0].action, CandidateAction::Reschedule);
    assert_eq!(b.candidates[0].time.precision, Precision::Exact);
}
#[test]
fn review_f2_complete_numeric_tokens_are_required() {
    for text in [
        "2026-10-123 09:00高数考试",
        "20266年10月12日09:00高数考试",
        "2026年10月12日109:00高数考试",
        "2026年10月12日09:000高数考试",
        "2026年10月12日9:00:30高数考试",
        "2026/10/123高数考试",
        "2026年110月12日高数考试",
        "2026年10月112日高数考试",
        "2026年10月12日09:00.5高数考试",
    ] {
        let t = parse_time(text, SENT, "Asia/Shanghai").unwrap();
        assert_eq!(t.precision, Precision::UnknownDate, "{text}");
        assert_eq!(t.start_at, None);
    }
}
fn inherited_notice() -> ExtractBatch {
    let original = envelope("2026年10月12日高数考试");
    let mut notice = envelope("原定高数考试改至10月14日9:00");
    notice.message_key = MessageKey::from_uuid(Uuid::from_u128(9));
    notice.reply_to = Some(original.message_key);
    extract(&notice, &[], &[original], "Asia/Shanghai").unwrap()
}
fn review_part(text: &str, id: u128) -> PartResult {
    let mut b = block(text);
    b.part_id = PartId::from_uuid(Uuid::from_u128(id));
    PartResult {
        part_id: b.part_id,
        status: PartStatus::Success,
        blocks: vec![b],
        reason_code: None,
    }
}
#[test]
fn review_f3_compatible_merge_preserves_inherited_current_time() {
    let original = inherited_notice();
    let c = &original.candidates[0];
    assert_eq!(c.time.precision, Precision::Exact);
    let merged = merge_parts(
        &original,
        &[review_part("高数考试改至2026年10月14日9:00", 3)],
    )
    .unwrap();
    assert_eq!(merged.candidates[0].time, c.time);
    assert_eq!(
        merged.candidates[0].target_message_key,
        c.target_message_key
    );
    assert_eq!(merged.candidates[0].candidate_key, c.candidate_key);
    assert_eq!(merged.source_order, original.source_order);
    let conflicting = merge_parts(
        &original,
        &[review_part("高数考试改至2026年10月15日9:00", 3)],
    )
    .unwrap();
    assert_eq!(
        conflicting.candidates[0].time.precision,
        Precision::UnknownDate
    );
}
#[test]
fn review_f3_context_role_is_strict_and_bound_to_established_target() {
    use shixu_core::notifications::extract::context_evidence_target;
    let original = inherited_notice();
    let historical = original.candidates[0].evidence.last().unwrap();
    assert_eq!(
        context_evidence_target(historical).unwrap(),
        original.candidates[0].target_message_key
    );
    for role in [
        "n6.body.utf8-bytes.1;n6_context_year_target=not-a-uuid",
        "n6.body.utf8-bytes.1;n6_context_year_target=00000000-0000-0000-0000-000000000099",
    ] {
        let mut forged = original.clone();
        forged.candidates[0]
            .evidence
            .last_mut()
            .unwrap()
            .engine_version = role.into();
        assert!(merge_parts(&forged, &[review_part("高数考试改至2026年10月14日9:00", 3)]).is_err());
    }
    let mut unbound = original;
    unbound.candidates[0].target_message_key = None;
    assert!(
        merge_parts(
            &unbound,
            &[review_part("高数考试改至2026年10月14日9:00", 3)]
        )
        .is_err()
    );
}
#[test]
fn review_f4_coordinated_positive_remainder_is_partial() {
    for text in [
        "2026年10月12日9:00高数考试，10:00英语考试",
        "2026年10月12日9:00高数考试，以及研究生答辩",
        "2026年10月12日9:00高数考试以及研究生答辩",
    ] {
        let b = batch(text);
        assert_eq!(b.candidates[0].time.precision, Precision::Exact, "{text}");
        assert!(
            b.part_results
                .iter()
                .any(|p| p.status == PartStatus::PartialParse
                    && p.blocks
                        .iter()
                        .any(|e| e.text == text
                            && e.quality_flags.contains(&QualityFlag::PartialSource))),
            "{text}"
        );
    }
}
#[test]
fn review_f5_page_and_nontext_clause_occurrences_do_not_collapse() {
    let first = block("2026年10月12日高数考试");
    let mut second = block("2026年10月13日高数考试");
    second.page_or_sheet = Some(PageOrSheet::Page { number: 2 });
    assert_eq!(
        extract(&envelope(""), &[first, second], &[], "Asia/Shanghai"),
        Err(AppError::Conflict)
    );
    let duplicate = block("2026年10月12日高数考试；2026年10月13日高数考试");
    assert_eq!(
        extract(&envelope(""), &[duplicate], &[], "Asia/Shanghai"),
        Err(AppError::Conflict)
    );
}
#[test]
fn review_f6_location_conflict_has_no_arrival_or_majority_winner() {
    let original = batch("2026年10月12日高数考试，地点：A");
    let b = review_part("2026年10月12日高数考试，地点：B", 3);
    let another_b = review_part("2026年10月12日高数考试，地点：B", 4);
    for parts in [[b.clone(), another_b.clone()], [another_b, b]] {
        let merged = merge_parts(&original, &parts).unwrap();
        assert_eq!(merged.candidates[0].location, None);
    }
    let empty = batch("2026年10月12日高数考试");
    let merged = merge_parts(&empty, &[review_part("2026年10月12日高数考试，地点：A", 3)]).unwrap();
    assert_eq!(merged.candidates[0].location.as_deref(), Some("A"));
}

#[test]
fn review_create_reply_year_enrichment_is_explicitly_deferred() {
    let original = envelope("2026年10月12日高数考试");
    let mut notice = envelope("10月14日9:00高数考试");
    notice.message_key = MessageKey::from_uuid(Uuid::from_u128(9));
    notice.reply_to = Some(original.message_key);
    let b = extract(&notice, &[], &[original], "Asia/Shanghai").unwrap();
    assert_eq!(b.candidates[0].action, CandidateAction::Create);
    assert_eq!(b.candidates[0].target_message_key, None);
    assert_eq!(b.candidates[0].time.precision, Precision::UnknownDate);
    assert!(
        b.part_results
            .iter()
            .any(|p| p.status == PartStatus::PartialParse
                && p.blocks.iter().any(|e| e.text == notice.text))
    );
}
#[test]
fn review_f3_forged_incoming_role_and_empty_merge_fail_closed() {
    let original = inherited_notice();
    let mut incoming = review_part("高数考试改至2026年10月14日9:00", 3);
    incoming.blocks[0].engine_version =
        "n6.body.utf8-bytes.1;n6_context_year_target=00000000-0000-0000-0000-000000000001".into();
    assert!(merge_parts(&original, &[incoming]).is_err());
    let mut tampered = original;
    tampered.candidates[0].target_message_key = None;
    assert_eq!(merge_parts(&tampered, &[]), Err(AppError::Conflict));
}

#[test]
fn review_f3_year_role_does_not_authorize_conflicting_current_fragments() {
    let original = envelope("2026年10月12日高数考试");
    let mut notice = envelope("原定高数考试改至10月14日9:00");
    notice.message_key = MessageKey::from_uuid(Uuid::from_u128(9));
    notice.reply_to = Some(original.message_key);
    let contradictory = block("高数考试改至10月15日9:00");
    let b = extract(&notice, &[contradictory], &[original], "Asia/Shanghai").unwrap();
    assert_eq!(b.candidates[0].time.precision, Precision::UnknownDate);
}
#[test]
fn review_f3_bound_cancel_year_inheritance_stays_supported() {
    let original = envelope("2026年10月12日高数考试");
    let mut notice = envelope("取消10月14日高数考试");
    notice.message_key = MessageKey::from_uuid(Uuid::from_u128(9));
    notice.reply_to = Some(original.message_key);
    let b = extract(&notice, &[], &[original], "Asia/Shanghai").unwrap();
    assert_eq!(b.candidates[0].action, CandidateAction::Cancel);
    assert_eq!(
        b.candidates[0].time.local_date.as_deref(),
        Some("2026-10-14")
    );
    let merged = merge_parts(&b, &[review_part("取消2026年10月14日高数考试", 3)]).unwrap();
    assert_eq!(merged.candidates[0].time, b.candidates[0].time);
}

#[test]
fn review_f4_event_word_in_location_is_metadata_not_extra_predicate() {
    let b = batch("后天20:00线上会议，地点：线上会议室");
    assert_eq!(b.candidates[0].location.as_deref(), Some("线上会议室"));
    assert_eq!(b.candidates[0].time.precision, Precision::Exact);
    assert!(
        b.part_results
            .iter()
            .all(|p| p.status == PartStatus::Success)
    );
}
#[test]
fn review_f4_supported_prefix_survives_negated_coordinated_remainder() {
    let text = "2026年10月12日9:00高数考试，后天不补考";
    let b = batch(text);
    assert_eq!(b.candidates.len(), 1);
    assert_eq!(b.candidates[0].title, "高数考试");
    assert_eq!(b.candidates[0].time.precision, Precision::Exact);
    assert!(
        b.part_results.iter().any(
            |p| p.status == PartStatus::PartialParse && p.blocks.iter().any(|e| e.text == text)
        )
    );
}

#[test]
fn review2_date_decimal_tail_and_sentence_dot_are_distinct() {
    for text in ["2026-10-12.5 09:00高数考试", "2026/10/12.5 09:00高数考试"] {
        let t = parse_time(text, SENT, "Asia/Shanghai").unwrap();
        assert_eq!(t.precision, Precision::UnknownDate, "{text}");
        assert_eq!(t.start_at, None);
        assert_eq!(t.raw_time_text, text);
        let b = batch(text);
        assert_eq!(b.candidates[0].time.precision, Precision::UnknownDate);
        let ev = &b.candidates[0].evidence[0];
        let Some(EvidenceLocation::TextSpan { start, end }) = ev.cell_range_or_bbox else {
            panic!("missing source span")
        };
        assert_eq!(
            text.get(start as usize..end as usize),
            Some(ev.text.as_str())
        );
    }
    let valid = parse_time("2026-10-12.高数考试", SENT, "Asia/Shanghai").unwrap();
    assert_eq!(valid.precision, Precision::DateOnly);
    assert_eq!(valid.local_date.as_deref(), Some("2026-10-12"));
}
#[test]
fn review2_clock_sentence_dot_does_not_admit_fraction_or_seconds() {
    for text in [
        "高数考试2026年10月12日9:00.",
        "高数考试2026年10月12日9:00.地点：A301",
    ] {
        let t = parse_time(text, SENT, "Asia/Shanghai").unwrap();
        assert_eq!(t.precision, Precision::Exact, "{text}");
        assert_eq!(t.start_at, Some(1_791_766_800_000));
        assert_eq!(t.raw_time_text, text);
    }
    for text in [
        "高数考试2026年10月12日9:00.5",
        "高数考试2026年10月12日9:00:30",
        "高数考试2026年10月12日9:000",
    ] {
        assert_eq!(
            parse_time(text, SENT, "Asia/Shanghai").unwrap().precision,
            Precision::UnknownDate,
            "{text}"
        );
    }
}
#[test]
fn review2_simple_conjunctions_preserve_leading_event_and_partial_body() {
    for text in [
        "2026年10月12日9:00高数考试和研究生答辩",
        "2026年10月12日9:00高数考试及研究生答辩",
        "2026年10月12日9:00协和高数考试和研究生答辩",
    ] {
        let b = batch(text);
        assert_eq!(b.candidates.len(), 1);
        assert_eq!(b.candidates[0].time.precision, Precision::Exact);
        assert_eq!(b.candidates[0].time.start_at, Some(1_791_766_800_000));
        assert_eq!(b.candidates[0].target_message_key, None);
        assert!(
            b.part_results
                .iter()
                .any(|p| p.status == PartStatus::PartialParse
                    && p.reason_code == Some(PartReason::PartialSource)
                    && p.blocks
                        .iter()
                        .any(|e| e.text == text
                            && e.quality_flags.contains(&QualityFlag::PartialSource))),
            "{text}"
        );
    }
}
#[test]
fn review2_conjunction_within_explicit_location_remains_metadata() {
    let b = batch("后天20:00线上会议，地点：协和会议室");
    assert_eq!(b.candidates[0].location.as_deref(), Some("协和会议室"));
    assert_eq!(b.candidates[0].time.precision, Precision::Exact);
    assert!(
        b.part_results
            .iter()
            .all(|p| p.status == PartStatus::Success)
    );
}

#[test]
fn d1_review1_unique_subject_in_multi_event_reply_establishes_target_and_year() {
    let original = envelope("2026年10月12日高数考试；2027年10月14日英语考试");
    let mut notice = envelope("高数考试改至10月13日9:00");
    notice.message_key = MessageKey::from_uuid(Uuid::from_u128(90));
    notice.reply_to = Some(original.message_key);
    let result = extract(
        &notice,
        &[],
        std::slice::from_ref(&original),
        "Asia/Shanghai",
    )
    .unwrap();
    assert_eq!(
        result.candidates[0].target_message_key,
        Some(original.message_key)
    );
    assert_eq!(
        result.candidates[0].time.local_date.as_deref(),
        Some("2026-10-13")
    );
    let historical = result.candidates[0].evidence.last().unwrap();
    assert_eq!(historical.text, "2026年10月12日高数考试");
    assert_eq!(
        shixu_core::notifications::extract::context_evidence_target(historical).unwrap(),
        Some(original.message_key)
    );
}
#[test]
fn d1_review1_duplicate_subject_context_stays_unbound_without_year_inheritance() {
    let original = envelope("2026年10月12日高数考试；2027年10月14日高数考试");
    let mut notice = envelope("高数考试改至10月13日9:00");
    notice.message_key = MessageKey::from_uuid(Uuid::from_u128(90));
    notice.reply_to = Some(original.message_key);
    let result = extract(&notice, &[], &[original], "Asia/Shanghai").unwrap();
    assert_eq!(result.candidates[0].target_message_key, None);
    assert_eq!(result.candidates[0].time.precision, Precision::UnknownDate);
    assert_eq!(result.candidates[0].evidence.len(), 1);
}
