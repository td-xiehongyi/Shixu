//! UI-thread subclass allocation owns matching WTS registration cleanup.
use super::{NativeVaultEvent, decode};
use shixu_core::contracts::{AppResult, error::AppError};
use std::{cell::Cell, sync::Arc};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::RemoteDesktop::{
        NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
    },
    UI::{
        Shell::{DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{WM_NCDESTROY, WM_POWERBROADCAST},
    },
};
const ID: usize = 0x5348_5654;
struct Registration {
    hwnd: HWND,
    notify: Arc<dyn Fn(NativeVaultEvent) + Send + Sync>,
    fail: Arc<dyn Fn() + Send + Sync>,
    registered: Cell<bool>,
}
impl Registration {
    fn close(&self) -> AppResult<()> {
        // SAFETY: allocation is released only by owning UI-thread uninstall or
        // WM_NCDESTROY. HWND remains live through this cleanup.
        let registered = self.registered.replace(false);
        if registered && unsafe { WTSUnRegisterSessionNotification(self.hwnd) } == 0 {
            (self.fail)();
            return Err(AppError::Unsupported);
        }
        Ok(())
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
/// # Safety
/// Caller must supply the retained trusted top-level HWND on its owning thread.
/// Allocation remains owned by the subclass until uninstall or WM_NCDESTROY.
/// No blanket Send/Sync is implemented for HWND ownership.
pub unsafe fn install(
    hwnd: HWND,
    notify: Arc<dyn Fn(NativeVaultEvent) + Send + Sync>,
    fail: Arc<dyn Fn() + Send + Sync>,
) -> AppResult<()> {
    if hwnd.is_null() {
        return Err(AppError::Unsupported);
    }
    let mut old = 0;
    // SAFETY: caller's live window; output is stack-owned.
    if unsafe { GetWindowSubclass(hwnd, Some(callback), ID, &mut old) } != 0 {
        return Err(AppError::Conflict);
    }
    let state = Box::new(Registration {
        hwnd,
        notify,
        fail,
        registered: Cell::new(false),
    });
    // SAFETY: actual top-level window, matching unregister held by allocation.
    if unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } == 0 {
        return Err(AppError::Unsupported);
    }
    state.registered.set(true);
    let state = Box::into_raw(state);
    // SAFETY: stable allocation transferred to fixed private subclass.
    if unsafe { SetWindowSubclass(hwnd, Some(callback), ID, state as usize) } == 0 {
        // SAFETY: subclass never installed, sole allocation owner.
        unsafe {
            drop(Box::from_raw(state));
        }
        return Err(AppError::Unsupported);
    }
    Ok(())
}
/// # Safety
/// Must execute on owning UI thread before window destruction. Failure retains
/// allocation for WM_NCDESTROY; never free a pointer still referenced by Windows.
pub unsafe fn uninstall(hwnd: HWND) -> AppResult<()> {
    let mut state = 0;
    // SAFETY: caller's live HWND and private matching callback/ID.
    if unsafe { GetWindowSubclass(hwnd, Some(callback), ID, &mut state) } == 0 {
        return Ok(());
    }
    if unsafe { RemoveWindowSubclass(hwnd, Some(callback), ID) } == 0 {
        // SAFETY: GetWindowSubclass returned our stable allocation.
        unsafe {
            ((*(state as *const Registration)).fail)();
        }
        return Err(AppError::Unsupported);
    }
    // SAFETY: successful removal released Windows's last reference; sole owner.
    let state = unsafe { Box::from_raw(state as *mut Registration) };
    state.close()
}
unsafe extern "system" fn callback(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: stable allocation owned by this installed subclass on this thread.
    let state = unsafe { &*(data as *const Registration) };
    // Defensively prevent callback capability panics crossing the FFI boundary.
    let event = decode(message, wparam);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Some(event) = event {
            (state.notify)(event);
        }
    }));
    if result.is_err() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (state.fail)()));
    }
    if message == WM_NCDESTROY {
        // Unregister while HWND is live, then forward destruction through the chain.
        let removed = unsafe { RemoveWindowSubclass(hwnd, Some(callback), ID) } != 0;
        let _ = state.close();
        if removed {
            unsafe {
                drop(Box::from_raw(data as *mut Registration));
            }
        } else {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (state.fail)()));
        }
        let forwarded = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
        // Failed removal retains allocation rather than risking use-after-free.
        return forwarded;
    }
    // SAFETY: forward every message through Tauri's existing subclass chain.
    let forwarded = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    if message == WM_POWERBROADCAST
        && matches!(
            event,
            Some(NativeVaultEvent::Suspend | NativeVaultEvent::Resume)
        )
    {
        1 // TRUE for handled power notifications, after forwarding the chain.
    } else {
        forwarded
    }
}
