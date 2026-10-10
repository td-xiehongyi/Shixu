// Synthetic write-only port tests; these do not establish Windows acceptance.
use shixu_core::contracts::{AppResult, error::AppError, vault::SecretBytes};
use shixu_native::vault::clipboard::{ClipboardPort, SystemClipboard, copy};
#[derive(Default)]
struct Memory {
    value: Vec<u8>,
    fail: bool,
}
impl ClipboardPort for Memory {
    fn write(&mut self, value: SecretBytes) -> AppResult<()> {
        if self.fail {
            return Err(AppError::Unsupported);
        }
        self.value = value.expose().to_vec();
        Ok(())
    }
}
#[test]
fn copy_preserves_unicode_whitespace_and_newlines() {
    let mut port = Memory::default();
    let value = "  合成🔐\npassword  ".as_bytes();
    copy(&mut port, SecretBytes::new(value.to_vec())).unwrap();
    assert_eq!(port.value, value);
}
#[test]
fn borrowed_copy_returns_without_clearing_clipboard() {
    let mut port = Memory::default();
    {
        let borrowed = &mut port;
        copy(borrowed, SecretBytes::new(vec![7])).unwrap();
    }
    assert_eq!(port.value, [7]);
    // Ending a borrowed copy scope cannot clear a subsequent user overwrite.
    {
        let borrowed = &mut port;
        copy(borrowed, SecretBytes::new(vec![7])).unwrap();
        borrowed.value = vec![8];
    }
    assert_eq!(port.value, [8]);
    // Manual clear is an external action, never a policy callback.
    port.value.clear();
    assert!(port.value.is_empty());
}
#[test]
fn repeat_copy_replaces_content_without_retaining_cleanup_authority() {
    let mut port = Memory::default();
    copy(&mut port, SecretBytes::new(vec![7])).unwrap();
    copy(&mut port, SecretBytes::new(vec![8])).unwrap();
    assert_eq!(port.value, [8]);
}
#[test]
fn failed_write_preserves_existing_user_content() {
    let mut port = Memory {
        value: vec![9],
        fail: true,
    };
    assert_eq!(
        copy(&mut port, SecretBytes::new(vec![7])),
        Err(AppError::Unsupported)
    );
    assert_eq!(port.value, [9]);
}
#[cfg(not(windows))]
#[test]
fn system_clipboard_is_explicitly_unsupported() {
    assert_eq!(
        copy(&mut SystemClipboard, SecretBytes::new(vec![7])),
        Err(AppError::Unsupported)
    );
}

// Exercises the same encoding and publication transaction used by Win32.
use shixu_native::vault::clipboard::publication::{self, Backend, Format};
#[derive(Default)]
struct Synthetic {
    calls: Vec<&'static str>,
    prepared: Vec<(Format, Vec<u8>)>,
    published: Vec<Format>,
    fail: Option<&'static str>,
    old_present: bool,
    fail_publish: Option<usize>,
}
impl Synthetic {
    fn step(&mut self, name: &'static str) -> AppResult<()> {
        self.calls.push(name);
        if self.fail == Some(name) {
            Err(AppError::Unsupported)
        } else {
            Ok(())
        }
    }
}
impl Backend for Synthetic {
    type Buffer = usize;
    fn prepare(&mut self, format: Format, bytes: &[u8]) -> AppResult<usize> {
        self.step("prepare")?;
        self.prepared.push((format, bytes.to_vec()));
        Ok(self.prepared.len() - 1)
    }
    fn open(&mut self) -> AppResult<()> {
        self.step("open")
    }
    fn empty(&mut self) -> AppResult<()> {
        self.step("empty")?;
        self.old_present = false;
        Ok(())
    }
    fn publish(&mut self, index: &mut usize) -> AppResult<()> {
        self.step("publish")?;
        if self.fail_publish == Some(*index) {
            return Err(AppError::Unsupported);
        }
        self.published.push(self.prepared[*index].0);
        Ok(())
    }
    fn close(&mut self) -> AppResult<()> {
        self.step("close")
    }
}
fn secret(bytes: &[u8]) -> SecretBytes {
    SecretBytes::new(bytes.to_vec())
}
#[test]
fn shared_writer_encodes_exact_unicode_and_publishes_exclusions_first() {
    let text = "  合成🔐\npassword\r\n  ";
    let mut b = Synthetic::default();
    publication::write(&mut b, secret(text.as_bytes())).unwrap();
    assert_eq!(
        b.published,
        [
            Format::ExcludeMonitor,
            Format::History,
            Format::Cloud,
            Format::UnicodeText
        ]
    );
    for (_, bytes) in &b.prepared[..3] {
        assert_eq!(bytes, &[0, 0, 0, 0]);
    }
    let expected: Vec<u8> = text
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    assert_eq!(b.prepared[3].1, expected);
    assert_eq!(
        b.calls,
        [
            "prepare", "prepare", "prepare", "prepare", "open", "empty", "publish", "publish",
            "publish", "publish", "close"
        ]
    );
}
#[test]
fn shared_writer_rejects_incompatible_or_oversized_input_before_touching_clipboard() {
    for value in [
        vec![0xff],
        b"synthetic\0tail".to_vec(),
        vec![b'x'; publication::MAX_BYTES + 1],
    ] {
        let mut b = Synthetic::default();
        assert!(publication::write(&mut b, secret(&value)).is_err());
        assert!(b.calls.is_empty());
    }
}
#[test]
fn shared_writer_empty_and_maximum_input_are_not_truncated() {
    for input in [vec![], vec![b'x'; publication::MAX_BYTES]] {
        let mut b = Synthetic::default();
        publication::write(&mut b, secret(&input)).unwrap();
        assert_eq!(b.prepared[3].1.len(), (input.len() + 1) * 2);
        assert_eq!(&b.prepared[3].1[b.prepared[3].1.len() - 2..], &[0, 0]);
    }
}
#[test]
fn preparation_and_open_failures_preserve_old_content_without_destructive_call() {
    for fail in ["prepare", "open"] {
        let mut b = Synthetic {
            fail: Some(fail),
            old_present: true,
            ..Default::default()
        };
        assert_eq!(
            publication::write(&mut b, secret(b"synthetic")),
            Err(AppError::Unsupported)
        );
        assert!(b.old_present);
        assert!(!b.calls.contains(&"empty"));
        assert!(!b.calls.contains(&"close"));
    }
}
#[test]
fn empty_failure_closes_without_publication() {
    let mut b = Synthetic {
        fail: Some("empty"),
        old_present: true,
        ..Default::default()
    };
    assert!(publication::write(&mut b, secret(b"synthetic")).is_err());
    assert!(b.old_present);
    assert!(b.published.is_empty());
    assert_eq!(b.calls.last(), Some(&"close"));
}
#[test]
fn every_publication_failure_stops_before_later_formats_and_does_not_rollback() {
    for index in 0..4 {
        let mut b = Synthetic {
            fail_publish: Some(index),
            old_present: true,
            ..Default::default()
        };
        assert!(publication::write(&mut b, secret(b"synthetic")).is_err());
        assert!(!b.old_present); // EmptyClipboard is not an atomic commit.
        assert_eq!(b.published.len(), index);
        assert!(!b.published.contains(&Format::UnicodeText));
        assert_eq!(b.calls.iter().filter(|c| **c == "empty").count(), 1);
        assert_eq!(b.calls.last(), Some(&"close"));
    }
}
#[test]
fn close_failure_reports_error_without_clearing_published_text() {
    let mut b = Synthetic {
        fail: Some("close"),
        ..Default::default()
    };
    assert!(publication::write(&mut b, secret(b"synthetic")).is_err());
    assert_eq!(b.published.last(), Some(&Format::UnicodeText));
    assert_eq!(b.calls.iter().filter(|c| **c == "empty").count(), 1);
}

/// Explicit opt-in destructive synthetic test. This is not the full clipboard gate.
/// Requires interactive Windows10/11, a disposable clipboard, no concurrent writer.
#[cfg(windows)]
#[test]
#[ignore = "BLOCKED until manually selected on an interactive Windows synthetic test account; overwrites clipboard"]
#[allow(unsafe_code)]
fn windows_synthetic_unicode_and_exclusion_formats_are_published() {
    use windows_sys::{
        Win32::{
            Foundation::GetLastError,
            System::{
                DataExchange::{
                    CloseClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
                },
                Memory::{GlobalLock, GlobalSize, GlobalUnlock},
            },
        },
        core::w,
    };
    struct Open;
    impl Drop for Open {
        fn drop(&mut self) {
            unsafe {
                CloseClipboard();
            }
        }
    }
    unsafe fn read(format: u32, expected: &[u8]) {
        let handle = unsafe { GetClipboardData(format) };
        assert!(!handle.is_null(), "synthetic format unavailable");
        assert!(unsafe { GlobalSize(handle) } >= expected.len());
        let memory = unsafe { GlobalLock(handle) };
        assert!(!memory.is_null(), "synthetic read lock failed");
        // Avoid asserting while locked; do not print clipboard bytes on failure.
        let equal =
            unsafe { std::slice::from_raw_parts(memory.cast::<u8>(), expected.len()) } == expected;
        unsafe {
            GlobalUnlock(handle);
        }
        assert!(equal, "synthetic clipboard bytes differ");
    }
    let text = "  合成🔐\npassword\r\n  ";
    copy(&mut SystemClipboard, secret(text.as_bytes())).unwrap();
    // Adapter owner has been destroyed: immediate data must remain readable.
    assert_ne!(
        unsafe { OpenClipboard(std::ptr::null_mut()) },
        0,
        "read open failed: {}",
        unsafe { GetLastError() }
    );
    let _open = Open;
    let expected: Vec<u8> = text
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    unsafe {
        read(13, &expected);
        for name in [
            w!("ExcludeClipboardContentFromMonitorProcessing"),
            w!("CanIncludeInClipboardHistory"),
            w!("CanUploadToCloudClipboard"),
        ] {
            let format = RegisterClipboardFormatW(name);
            assert_ne!(format, 0);
            read(format, &[0, 0, 0, 0]);
        }
    }
    // Deliberately no clearing: manually overwrite/clear using another application.
}

#[test]
fn lifecycle_revoked_during_preparation_preserves_previous_clipboard() {
    use std::cell::Cell;
    struct Revoking<'a> {
        inner: Synthetic,
        admitted: &'a Cell<bool>,
        revoke_on_open: bool,
    }
    impl Backend for Revoking<'_> {
        type Buffer = usize;
        fn prepare(&mut self, f: Format, bytes: &[u8]) -> AppResult<usize> {
            let result = self.inner.prepare(f, bytes);
            if f == Format::UnicodeText && !self.revoke_on_open {
                self.admitted.set(false);
            }
            result
        }
        fn open(&mut self) -> AppResult<()> {
            if self.revoke_on_open {
                self.admitted.set(false);
            }
            self.inner.open()
        }
        fn empty(&mut self) -> AppResult<()> {
            self.inner.empty()
        }
        fn publish(&mut self, b: &mut usize) -> AppResult<()> {
            self.inner.publish(b)
        }
        fn close(&mut self) -> AppResult<()> {
            self.inner.close()
        }
    }
    for revoke_on_open in [false, true] {
        let admitted = Cell::new(true);
        let mut backend = Revoking {
            inner: Synthetic {
                old_present: true,
                ..Default::default()
            },
            admitted: &admitted,
            revoke_on_open,
        };
        assert_eq!(
            publication::write_checked(&mut backend, secret(b"synthetic"), &|| if admitted.get() {
                Ok(())
            } else {
                Err(AppError::Locked)
            }),
            Err(AppError::Locked)
        );
        assert!(backend.inner.old_present);
        assert!(backend.inner.published.is_empty());
        assert!(!backend.inner.calls.contains(&"empty"));
        assert_eq!(backend.inner.calls.last(), Some(&"close"));
    }
}
