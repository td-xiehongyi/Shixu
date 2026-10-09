//! Portable ownership policy only. The real system/history adapter is explicitly unavailable.
use shixu_core::contracts::{AppResult, UtcMillis, error::AppError, vault::SecretBytes};
/// A write consumes secret ownership and returns a fresh, never-reused generation.
/// compare + clear MUST run atomically under the system clipboard lock. Comparing
/// bytes alone cannot authenticate same-content replacement by another program.
pub trait ClipboardPort {
    fn write_owned(&mut self, value: SecretBytes) -> AppResult<u64>;
    fn clear_generation(&mut self, generation: u64) -> AppResult<bool>;
}
pub struct ClipboardPolicy<P> {
    port: P,
    owned: Option<(u64, UtcMillis)>,
}
impl<P: ClipboardPort> ClipboardPolicy<P> {
    pub fn new(port: P) -> Self {
        Self { port, owned: None }
    }
    pub fn copy_owned(&mut self, value: SecretBytes, now: UtcMillis) -> AppResult<()> {
        let deadline = now.checked_add(30_000).ok_or(AppError::InvalidInput)?;
        let generation = self.port.write_owned(value)?;
        self.owned = Some((generation, deadline));
        Ok(())
    }
    pub fn clear_if_owned(&mut self, now: UtcMillis) -> AppResult<bool> {
        let Some((generation, deadline)) = self.owned else {
            return Ok(false);
        };
        if now < deadline {
            return Ok(false);
        }
        // Retain token on transient failure so a subsequent attempt can compare again.
        let cleared = self.port.clear_generation(generation)?;
        self.owned = None;
        Ok(cleared)
    }
    pub fn port(&self) -> &P {
        &self.port
    }
    pub fn port_mut(&mut self) -> &mut P {
        &mut self.port
    }
}
/// No navigator/browser fallback and no unverified Win32 history-suppression claim.
pub struct SystemClipboard;
impl ClipboardPort for SystemClipboard {
    fn write_owned(&mut self, _value: SecretBytes) -> AppResult<u64> {
        Err(AppError::Unsupported)
    }
    fn clear_generation(&mut self, _generation: u64) -> AppResult<bool> {
        Err(AppError::Unsupported)
    }
}
