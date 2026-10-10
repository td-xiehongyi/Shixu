// Write-only portable boundary; the production dispatcher remains unavailable.
use shixu_core::contracts::{AppResult, error::AppError, vault::SecretBytes};
/// Consumes transient secret memory. Clipboard content remains until the user
/// overwrites or manually clears it; no timer, lock, exit or drop cleanup exists.
/// Native history/cloud exclusion still requires implementation and acceptance.
pub trait ClipboardPort {
    fn write(&mut self, value: SecretBytes) -> AppResult<()>;
}
/// Borrow the clipboard only for this write; retain no ownership/cleanup token.
pub fn copy(port: &mut impl ClipboardPort, value: SecretBytes) -> AppResult<()> {
    port.write(value)
}
/// No navigator/browser fallback and no unverified Win32 history-suppression claim.
pub struct SystemClipboard;
impl ClipboardPort for SystemClipboard {
    fn write(&mut self, _value: SecretBytes) -> AppResult<()> {
        Err(AppError::Unsupported)
    }
}
