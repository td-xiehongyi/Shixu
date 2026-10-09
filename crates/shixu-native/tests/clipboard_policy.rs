use shixu_core::contracts::{AppResult, vault::SecretBytes};
use shixu_native::vault::clipboard::{ClipboardPolicy, ClipboardPort};
#[derive(Default)]
struct Memory {
    generation: u64,
    value: Option<SecretBytes>,
    clears: usize,
}
impl Memory {
    fn user_copy(&mut self) {
        self.generation += 1;
        self.value = Some(SecretBytes::new(vec![7]));
    }
}
impl ClipboardPort for Memory {
    fn write_owned(&mut self, value: SecretBytes) -> AppResult<u64> {
        self.generation += 1;
        self.value = Some(value);
        Ok(self.generation)
    }
    fn clear_generation(&mut self, generation: u64) -> AppResult<bool> {
        if self.generation != generation {
            return Ok(false);
        }
        self.value = None;
        self.generation += 1;
        self.clears += 1;
        Ok(true)
    }
}
#[test]
fn clipboard_clears_own_content_at_thirty_seconds() {
    let mut p = ClipboardPolicy::new(Memory::default());
    p.copy_owned(SecretBytes::new(vec![7]), 0).unwrap();
    assert!(!p.clear_if_owned(29999).unwrap());
    assert!(p.clear_if_owned(30000).unwrap());
    assert!(p.port().value.is_none());
    assert!(!p.clear_if_owned(60000).unwrap());
    assert_eq!(p.port().clears, 1);
}
#[test]
fn clipboard_new_content_survives_even_same_content() {
    let mut p = ClipboardPolicy::new(Memory::default());
    p.copy_owned(SecretBytes::new(vec![7]), 0).unwrap();
    p.port_mut().user_copy();
    assert!(!p.clear_if_owned(30000).unwrap());
    assert!(p.port().value.is_some());
    assert_eq!(p.port().clears, 0);
}
#[test]
fn repeat_copy_replaces_deadline() {
    let mut p = ClipboardPolicy::new(Memory::default());
    p.copy_owned(SecretBytes::new(vec![7]), 0).unwrap();
    p.copy_owned(SecretBytes::new(vec![8]), 20000).unwrap();
    assert!(!p.clear_if_owned(30000).unwrap());
    assert!(p.clear_if_owned(50000).unwrap());
}

#[test]
fn system_clipboard_is_explicitly_unsupported() {
    use shixu_core::contracts::error::AppError;
    use shixu_native::vault::clipboard::SystemClipboard;
    let mut p = ClipboardPolicy::new(SystemClipboard);
    assert_eq!(
        p.copy_owned(SecretBytes::new(vec![7]), 0),
        Err(AppError::Unsupported)
    );
}
