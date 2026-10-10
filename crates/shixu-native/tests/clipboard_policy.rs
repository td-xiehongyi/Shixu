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
#[test]
fn system_clipboard_is_explicitly_unsupported() {
    assert_eq!(
        copy(&mut SystemClipboard, SecretBytes::new(vec![7])),
        Err(AppError::Unsupported)
    );
}
