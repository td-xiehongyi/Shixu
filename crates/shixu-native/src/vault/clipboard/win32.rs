//! Immediate Win32 publication only. No read, ownership observation or clearing policy.
use super::{
    publication::{self, Backend, Format},
    *,
};
use std::ptr::{null, null_mut};
use windows_sys::{
    Win32::{
        Foundation::{GetLastError, GlobalFree, HGLOBAL, HWND, SetLastError},
        System::{
            DataExchange::{
                CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW,
                SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalUnlock},
        },
        UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE},
    },
    core::w,
};

// A non-delayed HGLOBAL is transferred exactly once. Drop only accesses buffers
// still owned here, never a published handle (or a subsequent user's content).
struct Buffer {
    handle: HGLOBAL,
    len: usize,
    format: u32,
}
impl Drop for Buffer {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: handle is an unpublished live GlobalAlloc allocation of
            // len bytes, with no other access. Published handles are set to null.
            unsafe {
                let memory = GlobalLock(self.handle);
                if !memory.is_null() {
                    use zeroize::Zeroize;
                    std::slice::from_raw_parts_mut(memory.cast::<u8>(), self.len).zeroize();
                    GlobalUnlock(self.handle);
                }
                GlobalFree(self.handle);
            }
        }
    }
}
struct Windows {
    owner: HWND,
    opened: bool,
}
impl Windows {
    fn new() -> AppResult<Self> {
        // Predefined STATIC class provides default processing. A message-only
        // window lives on this calling thread for the entire synchronous write.
        // Every format is immediately rendered; no message loop is required.
        // SAFETY: static NUL-terminated class/title; no secret window text or
        // user pointer. All HWND operations stay on the creating thread.
        let owner = unsafe {
            CreateWindowExW(
                0,
                w!("STATIC"),
                w!(""),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                null_mut(),
                null_mut(),
                null(),
            )
        };
        if owner.is_null() {
            return Err(AppError::Unsupported);
        }
        Ok(Self {
            owner,
            opened: false,
        })
    }
    fn finish(&mut self) -> AppResult<()> {
        self.close()?;
        if !self.owner.is_null() {
            if unsafe { DestroyWindow(self.owner) } == 0 {
                return Err(AppError::Unsupported);
            }
            self.owner = null_mut();
        }
        Ok(())
    }
}
impl Drop for Windows {
    fn drop(&mut self) {
        // Best-effort resource release after errors. No EmptyClipboard here.
        let _ = self.finish();
    }
}
impl Backend for Windows {
    type Buffer = Buffer;
    fn prepare(&mut self, format: Format, bytes: &[u8]) -> AppResult<Buffer> {
        let id = match format {
            Format::ExcludeMonitor => unsafe {
                RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"))
            },
            Format::History => unsafe {
                RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"))
            },
            Format::Cloud => unsafe { RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")) },
            Format::UnicodeText => 13, // CF_UNICODETEXT
        };
        if id == 0 {
            return Err(AppError::Unsupported);
        }
        // The shared writer bounds sizes and supplies only nonempty payloads.
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len()) };
        if handle.is_null() {
            return Err(AppError::Unsupported);
        }
        let buffer = Buffer {
            handle,
            len: bytes.len(),
            format: id,
        };
        // SAFETY: newly allocated, exclusively owned len-byte movable storage;
        // copy stays in bounds. The lock is released before publication.
        unsafe {
            let memory = GlobalLock(handle);
            if memory.is_null() {
                return Err(AppError::Unsupported);
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), memory.cast::<u8>(), bytes.len());
            SetLastError(0);
            // Zero with ERROR_SUCCESS means the final lock was released.
            if GlobalUnlock(handle) == 0 && GetLastError() != 0 {
                // Failed unlock: wipe while the original lock still protects
                // the pointer, then attempt release. Drop frees only this
                // unpublished allocation; never expose it to SetClipboardData.
                use zeroize::Zeroize;
                std::slice::from_raw_parts_mut(memory.cast::<u8>(), bytes.len()).zeroize();
                GlobalUnlock(handle);
                return Err(AppError::Unsupported);
            }
        }
        Ok(buffer)
    }
    fn open(&mut self) -> AppResult<()> {
        if unsafe { OpenClipboard(self.owner) } == 0 {
            return Err(AppError::Unsupported);
        }
        self.opened = true;
        Ok(())
    }
    fn empty(&mut self) -> AppResult<()> {
        if unsafe { EmptyClipboard() } == 0 {
            return Err(AppError::Unsupported);
        }
        Ok(())
    }
    fn publish(&mut self, buffer: &mut Buffer) -> AppResult<()> {
        // SAFETY: Open/Empty succeeded with a live owner; the prepared buffer
        // is unlocked and owned here. Never publish null/delayed-render data.
        if unsafe { SetClipboardData(buffer.format, buffer.handle) }.is_null() {
            return Err(AppError::Unsupported);
        }
        buffer.handle = null_mut(); // Windows now owns it; never wipe or free.
        Ok(())
    }
    fn close(&mut self) -> AppResult<()> {
        if self.opened {
            if unsafe { CloseClipboard() } == 0 {
                return Err(AppError::Unsupported);
            }
            self.opened = false;
        }
        Ok(())
    }
}
pub(super) fn write(value: SecretBytes) -> AppResult<()> {
    let mut backend = Windows::new()?;
    let result = publication::write(&mut backend, value);
    // Return cleanup errors even after successful publication. Do not roll back.
    let finished = backend.finish();
    result.and(finished)
}
