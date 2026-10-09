use shixu_core::{
    contracts::{AppResult, error::AppError},
    storage::{DataProtector, Database},
};
use std::path::Path;
/// Current Windows logon identity only; never CRYPTPROTECT_LOCAL_MACHINE.
/// No master password, external process, token DTO or plaintext fallback.
pub struct DpapiProtector;
impl DataProtector for DpapiProtector {
    fn protect(&self, plain: &[u8]) -> AppResult<Vec<u8>> {
        #[cfg(windows)]
        {
            win32::transform(plain, true)
        }
        #[cfg(not(windows))]
        {
            let _ = plain;
            Err(AppError::Unsupported)
        }
    }
    fn unprotect(&self, sealed: &[u8]) -> AppResult<Vec<u8>> {
        #[cfg(windows)]
        {
            win32::transform(sealed, false)
        }
        #[cfg(not(windows))]
        {
            let _ = sealed;
            Err(AppError::Unsupported)
        }
    }
}
impl DpapiProtector {
    /// Initialize an app-owned leaf directory with a protected, current-user-only
    /// inheritable ACL before creating SQLite/WAL/SHM files. Never pass a shared
    /// folder: this intentionally replaces the leaf's access rules.
    pub fn open_database(directory: &Path) -> AppResult<Database> {
        #[cfg(windows)]
        {
            if !directory.is_absolute() || directory.parent().is_none() {
                return Err(AppError::InvalidInput);
            }
            win32::private_directory(directory)?;
            for name in [
                "messages.sqlite3",
                "messages.sqlite3-wal",
                "messages.sqlite3-shm",
            ] {
                let path = directory.join(name);
                match std::fs::symlink_metadata(&path) {
                    Ok(_) => win32::private_file(&path)?,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err(AppError::Disconnected),
                }
            }
            Database::open(
                &directory.join("messages.sqlite3"),
                std::sync::Arc::new(Self),
            )
        }
        #[cfg(not(windows))]
        {
            let _ = directory;
            Err(AppError::Unsupported)
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod win32 {
    use super::*;
    use std::{
        ffi::c_void,
        os::windows::{ffi::OsStrExt, fs::MetadataExt},
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, ERROR_ALREADY_EXISTS, ERROR_INSUFFICIENT_BUFFER, GetLastError, HANDLE,
            LocalFree,
        },
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            },
            Cryptography::{
                CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
            },
            DACL_SECURITY_INFORMATION, GetTokenInformation, PROTECTED_DACL_SECURITY_INFORMATION,
            PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, SetFileSecurityW, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        Storage::FileSystem::{CreateDirectoryW, FILE_ATTRIBUTE_REPARSE_POINT},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    struct LocalAllocation(*mut c_void);
    impl Drop for LocalAllocation {
        fn drop(&mut self) {
            // SAFETY: these allocations come solely from Win32 LocalAlloc-family
            // APIs below, are owned once, and remain valid until this destructor.
            unsafe {
                if !self.0.is_null() {
                    LocalFree(self.0);
                }
            }
        }
    }
    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            // SAFETY: handle is returned by successful OpenProcessToken and uniquely
            // owned here. GetCurrentProcess's pseudo-handle is never wrapped/closed.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    pub(super) fn transform(input: &[u8], protect: bool) -> AppResult<Vec<u8>> {
        let len = u32::try_from(input.len()).map_err(|_| AppError::InvalidInput)?;
        let blob = CRYPT_INTEGER_BLOB {
            cbData: len,
            pbData: input.as_ptr().cast_mut(),
        };
        let entropy_bytes = b"shixu-local-data-v1";
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: entropy_bytes.len() as u32,
            pbData: entropy_bytes.as_ptr().cast_mut(),
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: null_mut(),
        };
        // SAFETY: input/entropy buffers and descriptors live through this
        // synchronous call; Win32 accepts borrowed input blobs and does not
        // mutate them. All optional pointers are null, UI is forbidden, output
        // is initialized and owned through LocalAllocation on every path.
        let success = unsafe {
            if protect {
                CryptProtectData(
                    &blob,
                    null(),
                    &entropy,
                    null(),
                    null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            } else {
                CryptUnprotectData(
                    &blob,
                    null_mut(),
                    &entropy,
                    null(),
                    null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            }
        };
        let allocation = LocalAllocation(output.pbData.cast());
        if success == 0 {
            return Err(AppError::AuthFailed);
        }
        if output.cbData == 0 {
            return Ok(Vec::new());
        }
        if allocation.0.is_null() {
            return Err(AppError::AuthFailed);
        }
        // SAFETY: successful DPAPI returns exactly cbData readable bytes in the
        // owned output allocation. Copy before freeing; zero decrypted Win32
        // buffer after copying so its LocalFree does not retain plain contents.
        let result = unsafe {
            let bytes = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
            let result = bytes.to_vec();
            if !protect {
                for byte in bytes {
                    std::ptr::write_volatile(byte, 0);
                }
            }
            result
        };
        Ok(result)
    }
    fn utf16(path: &Path) -> AppResult<Vec<u16>> {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(AppError::InvalidInput);
        }
        wide.push(0);
        Ok(wide)
    }
    fn descriptor() -> AppResult<LocalAllocation> {
        let mut handle = null_mut();
        // SAFETY: valid process pseudo-handle; output is initialized and becomes
        // a uniquely owned real token handle only after successful return.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
            return Err(AppError::AuthFailed);
        }
        let token = Token(handle);
        let mut required = 0;
        // SAFETY: documented two-call size query, null/zero output buffer.
        let first =
            unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut required) };
        if first != 0
            || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER
            || required < std::mem::size_of::<TOKEN_USER>() as u32
        {
            return Err(AppError::AuthFailed);
        }
        // usize storage ensures TOKEN_USER alignment and holds its inline SID.
        let words = (required as usize).div_ceil(std::mem::size_of::<usize>());
        let mut buffer = vec![0usize; words];
        // SAFETY: buffer has aligned allocated capacity >= queried size;
        // token/output-length pointers remain live for the synchronous call.
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(AppError::AuthFailed);
        }
        // SAFETY: successful TokenUser query initializes the header; its SID
        // points inside buffer, which stays alive until conversion completes.
        let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let mut sid_text = null_mut();
        // SAFETY: queried SID is valid; output is LocalAlloc-owned UTF-16.
        if unsafe { ConvertSidToStringSidW(sid, &mut sid_text) } == 0 {
            return Err(AppError::AuthFailed);
        }
        let sid_allocation = LocalAllocation(sid_text.cast());
        let mut length = 0;
        // SAFETY: successful conversion returns a null-terminated SID string.
        // Read its terminator within the Windows SID string's maximum bound.
        unsafe {
            while length < 256 && *sid_text.add(length) != 0 {
                length += 1;
            }
        }
        if length == 256 {
            return Err(AppError::AuthFailed);
        }
        // SAFETY: length measured in returned initialized UTF-16 allocation.
        let sid = unsafe { String::from_utf16(std::slice::from_raw_parts(sid_text, length)) }
            .map_err(|_| AppError::AuthFailed)?;
        drop(sid_allocation);
        let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;{sid})")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut raw: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: SDDL is null-terminated, revision 1 is documented; returned
        // self-relative descriptor is uniquely owned and LocalFree-released.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut raw,
                null_mut(),
            )
        } == 0
        {
            return Err(AppError::AuthFailed);
        }
        Ok(LocalAllocation(raw))
    }
    fn reject_reparse(path: &Path, directory: bool) -> AppResult<()> {
        let metadata = std::fs::symlink_metadata(path).map_err(|_| AppError::Disconnected)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || metadata.is_dir() != directory
        {
            return Err(AppError::InvalidInput);
        }
        Ok(())
    }
    pub(super) fn private_directory(path: &Path) -> AppResult<()> {
        let wide = utf16(path)?;
        let descriptor = descriptor()?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        // SAFETY: path/descriptor/attributes live for this call; directory is
        // created with its restrictive ACL atomically, no permissive interval.
        if unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) } == 0 {
            // SAFETY: immediate thread-local error read after failed Win32 call.
            if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
                return Err(AppError::Disconnected);
            }
        }
        reject_reparse(path, true)?;
        apply_acl(&wide, &descriptor)
    }
    pub(super) fn private_file(path: &Path) -> AppResult<()> {
        reject_reparse(path, false)?;
        apply_acl(&utf16(path)?, &descriptor()?)
    }
    fn apply_acl(path: &[u16], descriptor: &LocalAllocation) -> AppResult<()> {
        // SAFETY: checked null-terminated path and valid self-relative Win32
        // descriptor stay alive for call. Protect DACL against broad inheritance.
        if unsafe {
            SetFileSecurityW(
                path.as_ptr(),
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                descriptor.0,
            )
        } == 0
        {
            return Err(AppError::AuthFailed);
        }
        Ok(())
    }
}
