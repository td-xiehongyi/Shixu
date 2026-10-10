// Write-only native boundary; the production vault dispatcher remains unavailable.
use shixu_core::contracts::{AppResult, error::AppError, vault::SecretBytes};
/// Consumes transient secret memory. Clipboard content remains until the user
/// overwrites or manually clears it; no timer, lock, exit or drop cleanup exists.
/// Windows history/cloud exclusions are written but require native acceptance.
pub trait ClipboardPort {
    fn write(&mut self, value: SecretBytes) -> AppResult<()>;
}
/// Borrow the clipboard only for this write; retain no ownership/cleanup token.
pub fn copy(port: &mut impl ClipboardPort, value: SecretBytes) -> AppResult<()> {
    port.write(value)
}
/// No browser fallback. Windows source is written, not runtime accepted.
pub struct SystemClipboard;
impl ClipboardPort for SystemClipboard {
    fn write(&mut self, value: SecretBytes) -> AppResult<()> {
        #[cfg(windows)]
        {
            win32::write(value)
        }
        #[cfg(not(windows))]
        {
            let _ = value;
            Err(AppError::Unsupported)
        }
    }
}

/// Internal shared publication boundary, exposed for synthetic portable tests.
#[doc(hidden)]
pub mod publication {
    use super::*;
    pub const MAX_BYTES: usize = 65_536;
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Format {
        ExcludeMonitor,
        History,
        Cloud,
        UnicodeText,
    }
    pub trait Backend {
        type Buffer;
        fn prepare(&mut self, format: Format, bytes: &[u8]) -> AppResult<Self::Buffer>;
        fn open(&mut self) -> AppResult<()>;
        fn empty(&mut self) -> AppResult<()>;
        fn publish(&mut self, buffer: &mut Self::Buffer) -> AppResult<()>;
        fn close(&mut self) -> AppResult<()>;
    }
    /// All preparation precedes destructive emptying. Later failures can lose the
    /// previous content or leave exclusions/text published; never roll back.
    pub fn write<B: Backend>(backend: &mut B, value: SecretBytes) -> AppResult<()> {
        use zeroize::Zeroizing;
        let bytes = value.expose();
        if bytes.len() > MAX_BYTES {
            return Err(AppError::InvalidInput);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| AppError::Unsupported)?;
        if text.contains('\0') {
            return Err(AppError::Unsupported);
        }
        // At most (MAX_BYTES+1)*2 bytes. No trimming or newline normalization.
        // Preallocate the maximum required capacity before filling, so growth
        // cannot release a previous allocation containing secret staging bytes.
        let mut encoded = Zeroizing::new(Vec::with_capacity((bytes.len() + 1) * 2));
        encoded.extend(text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
        let mut buffers = [
            backend.prepare(Format::ExcludeMonitor, &0u32.to_le_bytes())?,
            backend.prepare(Format::History, &0u32.to_le_bytes())?,
            backend.prepare(Format::Cloud, &0u32.to_le_bytes())?,
            backend.prepare(Format::UnicodeText, &encoded)?,
        ];
        // Release staging and original secret before OpenClipboard.
        drop(encoded);
        drop(value);
        backend.open()?;
        let result = backend.empty().and_then(|()| {
            for buffer in &mut buffers {
                backend.publish(buffer)?;
            }
            Ok(())
        });
        let closed = backend.close();
        result.and(closed)
    }
}

#[cfg(windows)]
#[allow(unsafe_code)] // Narrow Win32 FFI boundary, matching the existing DPAPI adapter.
mod win32;
