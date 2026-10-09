use shixu_core::{
    contracts::{AppResult, error::AppError, notification::*},
    storage::DataProtector,
};
use shixu_native::attachments::{
    container::inspect_container,
    download::*,
    host::{self, DetectedType},
};
use std::{
    io::{Cursor, Write},
    path::PathBuf,
    sync::Arc,
};
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
struct FrozenClock;
impl Clock for FrozenClock {
    fn now(&self) -> i64 {
        1000
    }
}
struct Transport {
    calls: u32,
    expired: bool,
    thumbnail: bool,
    responses: std::collections::VecDeque<Result<Response, PartReason>>,
}
impl DownloadPort for Transport {
    fn resolve(&mut self, _r: &AttachmentRef) -> Result<Grant, PartReason> {
        Ok(Grant {
            endpoint: Endpoint {
                host: "files.example.test".into(),
                path: "/synthetic".into(),
            },
            expires_at: if self.expired { 1000 } else { 2000 },
            thumbnail_only: self.thumbnail,
        })
    }
    fn get(&mut self, _e: &Endpoint) -> Result<Response, PartReason> {
        self.calls += 1;
        self.responses.pop_front().unwrap()
    }
}
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("shixu-n3-{}", Uuid::new_v4())))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn reference() -> AttachmentRef {
    AttachmentRef {
        part_id: PartId::from_uuid(Uuid::new_v4()),
        message_key: MessageKey::from_uuid(Uuid::new_v4()),
        source_file_ref: Some("trusted-reference".to_owned().try_into().unwrap()),
        content_hash: None,
        encrypted_blob_ref: None,
        fetch_state: FetchState::Pending,
        retained_until: None,
    }
}
fn service(
    t: &Temp,
    responses: Vec<Result<Response, PartReason>>,
) -> DownloadService<Transport, FrozenClock> {
    DownloadService::new(
        Transport {
            calls: 0,
            expired: false,
            thumbnail: false,
            responses: responses.into(),
        },
        FrozenClock,
        ProtectedCache::create(&t.0, Arc::new(SyntheticProtector)).unwrap(),
        vec!["files.example.test".into()],
    )
}
#[test]
fn reject_redirect_to_unapproved_host() {
    let t = Temp::new();
    let mut s = service(
        &t,
        vec![Ok(Response::Redirect(Endpoint {
            host: "attacker.example.test".into(),
            path: "/x".into(),
        }))],
    );
    assert_eq!(
        s.fetch_detailed(&reference(), &ParserLimits::v01()),
        Err(PartReason::PermissionDenied)
    );
    assert_eq!(s.transport.calls, 1);
}
#[test]
fn expired_reference_is_visible() {
    let t = Temp::new();
    let mut s = service(&t, vec![]);
    s.transport.expired = true;
    assert_eq!(
        s.fetch_detailed(&reference(), &ParserLimits::v01()),
        Err(PartReason::Expired)
    );
    assert_eq!(s.transport.calls, 0);
    s.transport.expired = false;
    s.transport.thumbnail = true;
    assert_eq!(
        s.fetch_detailed(&reference(), &ParserLimits::v01()),
        Err(PartReason::PartialSource)
    );
    assert_eq!(s.transport.calls, 0);
}
fn archive(name: &str, content: &[u8]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    w.start_file("[Content_Types].xml", options).unwrap();
    w.write_all(b"synthetic types").unwrap();
    w.start_file(name, options).unwrap();
    w.write_all(content).unwrap();
    w.finish().unwrap().into_inner()
}
#[test]
fn zip_bomb_and_traversal_blocked() {
    let l = ParserLimits::v01();
    assert_eq!(
        inspect_container(&archive("word/document.xml", b"synthetic doc"), &l),
        Ok(DetectedType::Docx)
    );
    assert!(inspect_container(&archive("../escape", b"x"), &l).is_err());
    assert_eq!(
        inspect_container(&archive("word/document.xml", &vec![b'A'; 1024 * 1024]), &l),
        Err(PartReason::LimitExceeded)
    );
    assert_eq!(
        inspect_container(b"not a zip", &l),
        Err(PartReason::FormatUnsupported)
    );
}
#[test]
fn successful_download_is_protected_and_actual_stream_limit_is_enforced() {
    let t = Temp::new();
    let plain = b"%PDF-1.7\nSYNTHETIC-ATTACHMENT";
    let mut s = service(&t, vec![Ok(Response::Body(Box::new(Cursor::new(plain))))]);
    let path = s
        .fetch_attachment(&reference(), &ParserLimits::v01())
        .unwrap();
    assert!(path.starts_with(&t.0));
    let sealed = std::fs::read(path).unwrap();
    assert_ne!(sealed, plain);
    assert_eq!(SyntheticProtector.unprotect(&sealed).unwrap(), plain);
    let mut limits = ParserLimits::v01();
    limits.max_file_bytes = 8;
    let t = Temp::new();
    let mut s = service(&t, vec![Ok(Response::Body(Box::new(Cursor::new(plain))))]);
    assert_eq!(
        s.fetch_detailed(&reference(), &limits),
        Err(PartReason::LimitExceeded)
    );
}
#[test]
fn host_fails_closed_without_spawning_an_unsandboxed_parser() {
    let p:MessagePart=serde_json::from_value(serde_json::json!({"part_id":Uuid::new_v4(),"message_key":Uuid::new_v4(),"kind":"file","source_file_ref":"synthetic","original_name":null,"declared_type":null,"detected_type":"application/pdf","byte_size":10,"content_hash":null,"fetch_state":"fetched","parse_state":"fetched","failure_code":null,"encrypted_blob_ref":null,"retained_until":null})).unwrap();
    assert_eq!(
        host::parse_part(&p, &ParserLimits::v01()),
        Err(AppError::Unsupported)
    );
}
#[test]
#[ignore = "BLOCKED: requires implemented Windows restricted token, ACL, Job Object and real network/vault probes"]
fn parser_has_no_network_or_vault_access() {
    panic!(
        "BLOCKED: no validated Windows sandbox implementation; real outbound and controlled vault-directory probes have not run"
    );
}

#[test]
fn protected_cache_enforces_message_parts_and_reopen_1gib_quota() {
    let t = Temp::new();
    let body = b"%PDF-1.7";
    let mut s = service(
        &t,
        (0..6)
            .map(|_| {
                Ok(Response::Body(
                    Box::new(Cursor::new(body)) as Box<dyn std::io::Read>
                ))
            })
            .collect(),
    );
    let mut r = reference();
    for _ in 0..5 {
        r.part_id = PartId::from_uuid(Uuid::new_v4());
        s.fetch_detailed(&r, &ParserLimits::v01()).unwrap();
    }
    r.part_id = PartId::from_uuid(Uuid::new_v4());
    assert_eq!(
        s.fetch_detailed(&r, &ParserLimits::v01()),
        Err(PartReason::LimitExceeded)
    );
    drop(s);
    let pad = std::fs::File::create(t.0.join("reserved.blob")).unwrap();
    pad.set_len(1024 * 1024 * 1024 - 5 * body.len() as u64)
        .unwrap();
    let mut s = service(&t, vec![Ok(Response::Body(Box::new(Cursor::new(body))))]);
    assert_eq!(
        s.fetch_detailed(&reference(), &ParserLimits::v01()),
        Err(PartReason::StorageFull)
    );
}
#[test]
fn protected_cache_enforces_actual_message_total_at_and_over() {
    let t = Temp::new();
    let size = 10 * 1024 * 1024;
    let mut body = vec![0; size];
    body[..5].copy_from_slice(b"%PDF-");
    let mut s = service(
        &t,
        (0..6)
            .map(|_| {
                Ok(Response::Body(
                    Box::new(Cursor::new(body.clone())) as Box<dyn std::io::Read>
                ))
            })
            .collect(),
    );
    let mut r = reference();
    for _ in 0..5 {
        r.part_id = PartId::from_uuid(Uuid::new_v4());
        s.fetch_detailed(&r, &ParserLimits::v01()).unwrap();
    }
    r.part_id = PartId::from_uuid(Uuid::new_v4());
    assert_eq!(
        s.fetch_detailed(&r, &ParserLimits::v01()),
        Err(PartReason::LimitExceeded)
    );
}

#[test]
fn duplicate_central_directory_members_are_not_hidden_by_zip_index() {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for name in [
        "[Content_Types].xml",
        "word/document.xml",
        "word/document.xm2",
    ] {
        w.start_file(name, opts).unwrap();
        w.write_all(b"synthetic").unwrap();
    }
    let mut bytes = w.finish().unwrap().into_inner();
    for i in 0..bytes.len() - 17 {
        if &bytes[i..i + 17] == b"word/document.xm2" {
            bytes[i + 16] = b'l';
        }
    }
    assert_eq!(
        inspect_container(&bytes, &ParserLimits::v01()),
        Err(PartReason::FormatUnsupported)
    );
}

#[test]
fn cache_exact_1gib_boundary_is_allowed_then_one_more_byte_is_blocked() {
    let t = Temp::new();
    let mut s = service(
        &t,
        vec![Ok(Response::Body(Box::new(Cursor::new(b"%PDF-1.7"))))],
    );
    let pad = std::fs::File::create(t.0.join("reserved.blob")).unwrap();
    pad.set_len(1024 * 1024 * 1024 - 8).unwrap();
    s.fetch_detailed(&reference(), &ParserLimits::v01())
        .unwrap();
    drop(s);
    let mut s = service(
        &t,
        vec![Ok(Response::Body(Box::new(Cursor::new(b"%PDF-1.7"))))],
    );
    assert_eq!(
        s.fetch_detailed(&reference(), &ParserLimits::v01()),
        Err(PartReason::StorageFull)
    );
}
#[test]
fn actual_message_bytes_are_checked_even_below_five_parts() {
    let t = Temp::new();
    let size = 20 * 1024 * 1024;
    let mut body = vec![0; size];
    body[..5].copy_from_slice(b"%PDF-");
    let mut s = service(
        &t,
        (0..3)
            .map(|_| {
                Ok(Response::Body(
                    Box::new(Cursor::new(body.clone())) as Box<dyn std::io::Read>
                ))
            })
            .collect(),
    );
    let mut r = reference();
    for _ in 0..2 {
        r.part_id = PartId::from_uuid(Uuid::new_v4());
        s.fetch_detailed(&r, &ParserLimits::v01()).unwrap();
    }
    r.part_id = PartId::from_uuid(Uuid::new_v4());
    assert_eq!(
        s.fetch_detailed(&r, &ParserLimits::v01()),
        Err(PartReason::LimitExceeded)
    );
}
#[test]
fn missing_original_auth_failure_and_thumbnail_have_distinct_durable_states() {
    use shixu_native::attachments::download::download_failure;
    let p = reference().part_id;
    for (reason, status) in [
        (PartReason::Expired, PartStatus::DownloadFailed),
        (PartReason::AuthRequired, PartStatus::DownloadFailed),
        (PartReason::PartialSource, PartStatus::PartialParse),
        (PartReason::FormatUnsupported, PartStatus::Unsupported),
        (PartReason::LimitExceeded, PartStatus::LimitExceeded),
    ] {
        let r = download_failure(p, reason);
        assert_eq!(r.status, status);
        assert_eq!(r.reason_code, Some(reason));
        assert!(r.blocks.is_empty());
    }
    let t = Temp::new();
    let mut s = service(&t, vec![Err(PartReason::AuthRequired)]);
    assert_eq!(
        s.fetch_detailed(&reference(), &ParserLimits::v01()),
        Err(PartReason::AuthRequired)
    );
    assert_eq!(s.transport.calls, 1);
    let mut r = reference();
    r.source_file_ref = None;
    assert_eq!(
        s.fetch_detailed(&r, &ParserLimits::v01()),
        Err(PartReason::DownloadUnavailable)
    );
    assert_eq!(s.transport.calls, 1);
}
#[test]
fn injected_transport_cannot_choose_paths_and_child_protocol_rejects_commands() {
    for invalid in [
        "/etc/passwd",
        "https://attacker.test/p",
        "C:\\vault",
        "../file",
    ] {
        assert!(SourceFileRef::try_from(invalid.to_owned()).is_err());
    }
    let request = serde_json::json!({"part_id":Uuid::new_v4(),"input_handle":7,"detected_type":"pdf","limits":ParserLimits::v01(),"command":"open vault"});
    assert!(serde_json::from_value::<host::ParseRequest>(request).is_err());
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_shixu-parser"))
        .arg("--open-anything")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(78));
    assert!(out.stdout.is_empty());
    assert_eq!(out.stderr, b"UNSUPPORTED\n");
}
#[cfg(unix)]
#[test]
fn cache_rejects_symlink_and_serializes_ownership() {
    let t = Temp::new();
    let cache = ProtectedCache::create(&t.0, Arc::new(SyntheticProtector)).unwrap();
    assert!(matches!(
        ProtectedCache::create(&t.0, Arc::new(SyntheticProtector)),
        Err(AppError::Conflict)
    ));
    drop(cache);
    let other = Temp::new();
    std::os::unix::fs::symlink(&t.0, &other.0).unwrap();
    assert!(matches!(
        ProtectedCache::create(&other.0, Arc::new(SyntheticProtector)),
        Err(AppError::InvalidInput)
    ));
    std::fs::remove_file(&other.0).unwrap();
}

#[test]
fn forged_declared_uncompressed_size_is_rejected() {
    let mut bytes = archive("word/document.xml", b"synthetic document");
    for i in 0..bytes.len() - 46 {
        if &bytes[i..i + 4] == b"PK\x01\x02"
            && bytes.get(i + 46..i + 63) == Some(b"word/document.xml")
        {
            bytes[i + 24..i + 28].copy_from_slice(&1u32.to_le_bytes());
        }
    }
    assert!(inspect_container(&bytes, &ParserLimits::v01()).is_err());
}

#[test]
fn padded_declared_compressed_size_cannot_weaken_ratio_accounting() {
    let mut bytes = archive("word/document.xml", b"synthetic document");
    let old_end = bytes.windows(4).rposition(|w| w == b"PK\x05\x06").unwrap();
    let central =
        u32::from_le_bytes(bytes[old_end + 16..old_end + 20].try_into().unwrap()) as usize;
    bytes.insert(central, 0);
    let end = old_end + 1;
    bytes[end + 16..end + 20].copy_from_slice(&((central + 1) as u32).to_le_bytes());
    for i in 0..bytes.len() - 63 {
        if &bytes[i..i + 4] == b"PK\x01\x02"
            && bytes.get(i + 46..i + 63) == Some(b"word/document.xml")
        {
            let size = u32::from_le_bytes(bytes[i + 20..i + 24].try_into().unwrap());
            bytes[i + 20..i + 24].copy_from_slice(&(size + 1).to_le_bytes());
        }
        if &bytes[i..i + 4] == b"PK\x03\x04"
            && bytes.get(i + 30..i + 47) == Some(b"word/document.xml")
        {
            let size = u32::from_le_bytes(bytes[i + 18..i + 22].try_into().unwrap());
            bytes[i + 18..i + 22].copy_from_slice(&(size + 1).to_le_bytes());
        }
    }
    assert!(inspect_container(&bytes, &ParserLimits::v01()).is_err());
}

#[test]
fn review_cache_reacquisition_after_reopen_reuses_exact_complete_original() {
    let t = Temp::new();
    let r = reference();
    let body = b"%PDF-1.7 SYNTHETIC";
    let mut s = service(&t, vec![Ok(Response::Body(Box::new(Cursor::new(body))))]);
    let first = s.fetch_detailed(&r, &ParserLimits::v01()).unwrap();
    drop(s);
    // Simulates publication completed before task completion was durable.
    let before = std::fs::read(&first).unwrap();
    let mut s = service(&t, vec![Ok(Response::Body(Box::new(Cursor::new(body))))]);
    assert_eq!(
        s.fetch_detailed(&r, &ParserLimits::v01()),
        Ok(first.clone())
    );
    assert_eq!(std::fs::read(first).unwrap(), before);
}
#[test]
fn review_cache_reused_part_edit_keeps_old_original_and_identifies_new_content() {
    let t = Temp::new();
    let mut r = reference();
    let mut s = service(
        &t,
        vec![
            Ok(Response::Body(Box::new(Cursor::new(b"%PDF-1.7 OLD")))),
            Ok(Response::Body(Box::new(Cursor::new(b"%PDF-1.7 NEW")))),
            Ok(Response::Body(Box::new(Cursor::new(b"%PDF-1.7 NEW")))),
        ],
    );
    let old = s.fetch_detailed(&r, &ParserLimits::v01()).unwrap();
    let old_bytes = std::fs::read(&old).unwrap();
    r.source_file_ref = Some("edited-trusted-ref".to_owned().try_into().unwrap());
    let new = s.fetch_detailed(&r, &ParserLimits::v01()).unwrap();
    assert_ne!(old, new);
    assert_eq!(std::fs::read(&old).unwrap(), old_bytes);
    assert_eq!(
        SyntheticProtector
            .unprotect(&std::fs::read(&new).unwrap())
            .unwrap(),
        b"%PDF-1.7 NEW"
    );
    assert_eq!(s.fetch_detailed(&r, &ParserLimits::v01()), Ok(new));
}
#[test]
fn review_cache_reuse_does_not_consume_sixth_part_or_add_cache_bytes() {
    let t = Temp::new();
    let mut r = reference();
    let body = b"%PDF-1.7";
    let mut s = service(
        &t,
        (0..6)
            .map(|_| {
                Ok(Response::Body(
                    Box::new(Cursor::new(body)) as Box<dyn std::io::Read>
                ))
            })
            .collect(),
    );
    let mut last = PathBuf::new();
    for _ in 0..5 {
        r.part_id = PartId::from_uuid(Uuid::new_v4());
        last = s.fetch_detailed(&r, &ParserLimits::v01()).unwrap();
    }
    let pad = std::fs::File::create(t.0.join("reserved.blob")).unwrap();
    pad.set_len(1024 * 1024 * 1024 - 5 * body.len() as u64)
        .unwrap();
    assert_eq!(s.fetch_detailed(&r, &ParserLimits::v01()), Ok(last));
}
#[test]
fn review_cache_recovers_unpublished_partial_and_keeps_published_original() {
    let t = Temp::new();
    let r = reference();
    let body = b"%PDF-1.7 ORIGINAL";
    let mut s = service(&t, vec![Ok(Response::Body(Box::new(Cursor::new(body))))]);
    let original = s.fetch_detailed(&r, &ParserLimits::v01()).unwrap();
    drop(s);
    let partial = t.0.join(format!(".pending-{}", Uuid::new_v4()));
    std::fs::write(&partial, b"synthetic incomplete protected write").unwrap();
    let published_pending = t.0.join(format!(".pending-{}", Uuid::new_v4()));
    std::fs::hard_link(&original, &published_pending).unwrap();
    let mut s = service(&t, vec![Ok(Response::Body(Box::new(Cursor::new(body))))]);
    assert!(!partial.exists());
    assert!(!published_pending.exists());
    assert!(original.exists());
    assert_eq!(s.fetch_detailed(&r, &ParserLimits::v01()), Ok(original));
}
#[test]
fn review_cache_never_blindly_reuses_a_corrupted_complete_blob() {
    let t = Temp::new();
    let r = reference();
    let body = b"%PDF-1.7 ORIGINAL";
    let mut s = service(
        &t,
        vec![
            Ok(Response::Body(Box::new(Cursor::new(body)))),
            Ok(Response::Body(Box::new(Cursor::new(body)))),
        ],
    );
    let original = s.fetch_detailed(&r, &ParserLimits::v01()).unwrap();
    std::fs::write(&original, vec![0u8; body.len()]).unwrap();
    assert_eq!(
        s.fetch_detailed(&r, &ParserLimits::v01()),
        Err(PartReason::RecognitionFailed)
    );
    assert!(original.exists());
}

#[test]
fn review_cache_edit_at_five_logical_parts_preserves_all_prior_originals() {
    let t = Temp::new();
    let mut r = reference();
    let body = b"%PDF-1.7";
    let mut responses: Vec<_> = (0..5)
        .map(|_| {
            Ok(Response::Body(
                Box::new(Cursor::new(body)) as Box<dyn std::io::Read>
            ))
        })
        .collect();
    responses.push(Ok(Response::Body(Box::new(Cursor::new(b"%PDF-1.7 EDIT")))));
    let mut s = service(&t, responses);
    let mut originals = Vec::new();
    for _ in 0..5 {
        r.part_id = PartId::from_uuid(Uuid::new_v4());
        originals.push(s.fetch_detailed(&r, &ParserLimits::v01()).unwrap());
    }
    r.source_file_ref = Some(
        "new-trusted-ref-sensitive-marker"
            .to_owned()
            .try_into()
            .unwrap(),
    );
    let edited = s.fetch_detailed(&r, &ParserLimits::v01()).unwrap();
    assert!(!originals.contains(&edited));
    assert!(originals.iter().all(|p| p.exists()));
    assert!(!edited.to_string_lossy().contains("sensitive-marker"));
}
#[test]
fn d5_database_pause_blocks_cache_publication_and_startup_cleanup() {
    let p = Arc::new(SyntheticProtector);
    let db =
        shixu_core::storage::Database::open(std::path::Path::new(":memory:"), p.clone()).unwrap();
    let t = Temp::new();
    let cache = ProtectedCache::create_coordinated(&t.0, p.clone(), &db).unwrap();
    let mut s = DownloadService::new(
        Transport {
            calls: 0,
            expired: false,
            thumbnail: false,
            responses: vec![Ok(Response::Body(Box::new(Cursor::new(
                b"%PDF-synthetic".to_vec(),
            ))))]
            .into(),
        },
        FrozenClock,
        cache,
        vec!["files.example.test".into()],
    );
    let pause = db.pause_writes().unwrap();
    assert_eq!(
        s.fetch_detailed(&reference(), &ParserLimits::v01()),
        Err(PartReason::PermissionDenied)
    );
    drop(s);
    let pending = t.0.join(format!(".pending-{}", Uuid::new_v4()));
    std::fs::write(&pending, b"ciphertext-only").unwrap();
    assert!(matches!(
        ProtectedCache::create_coordinated(&t.0, p.clone(), &db),
        Err(AppError::Conflict)
    ));
    assert!(pending.exists());
    drop(pause);
    let _cache = ProtectedCache::create_coordinated(&t.0, p, &db).unwrap();
    assert!(!pending.exists());
}
