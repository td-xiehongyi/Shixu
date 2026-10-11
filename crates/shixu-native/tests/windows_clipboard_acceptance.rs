#![cfg(windows)]
// Native metadata checks only; never read the user's existing clipboard text.
use shixu_core::contracts::vault::SecretBytes;
use shixu_native::vault::clipboard::{ClipboardPort, SystemClipboard};
use windows_sys::Win32::System::{
    DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW,
    },
    Memory::{GlobalLock, GlobalSize, GlobalUnlock},
};

#[test]
#[ignore = "Writes only synthetic canaries to the actual interactive Windows clipboard"]
fn actual_clipboard_publishes_history_and_cloud_exclusions() {
    for value in [
        "SHIXU-SYNTHETIC-ACCOUNT@example.invalid",
        "SHIXU-SYNTHETIC-PASSWORD-仅测试",
    ] {
        SystemClipboard
            .write(SecretBytes::new(value.as_bytes().to_vec()))
            .unwrap();
        unsafe {
            // The shell may briefly inspect newly published formats. Only the
            // observer retries; the production writer and its checks stay intact.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                if OpenClipboard(std::ptr::null_mut()) != 0 {
                    break;
                }
                let error = windows_sys::Win32::Foundation::GetLastError();
                assert!(
                    std::time::Instant::now() < deadline,
                    "clipboard observer open failed: {error}"
                );
                eprintln!("clipboard observer contention: {error}");
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let checked = std::panic::catch_unwind(|| {
                for name in [
                    "ExcludeClipboardContentFromMonitorProcessing",
                    "CanIncludeInClipboardHistory",
                    "CanUploadToCloudClipboard",
                ] {
                    let wide: Vec<_> = name.encode_utf16().chain([0]).collect();
                    let format = RegisterClipboardFormatW(wide.as_ptr());
                    assert_ne!(format, 0);
                    assert_ne!(
                        IsClipboardFormatAvailable(format),
                        0,
                        "missing exclusion format"
                    );
                    let handle = GetClipboardData(format);
                    assert!(!handle.is_null());
                    assert!(GlobalSize(handle) >= 4);
                    let memory = GlobalLock(handle);
                    assert!(!memory.is_null());
                    let zero = std::ptr::read_unaligned(memory.cast::<u32>()) == 0;
                    GlobalUnlock(handle);
                    assert!(zero, "history/cloud exclusion must contain DWORD zero");
                }
                assert_ne!(
                    IsClipboardFormatAvailable(13),
                    0,
                    "native Unicode text format missing"
                );
            });
            let close = CloseClipboard();
            assert_ne!(close, 0);
            if let Err(panic) = checked {
                std::panic::resume_unwind(panic);
            }
        }
    }
}
