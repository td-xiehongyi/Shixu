//! Private Win32 boundary. Production remains blocked until native acceptance.
//! Regular AppContainer permits shared system/profile surfaces; it is not LPAC.
//! FFI pointers borrow live UTF-16, SID, descriptor, aligned query and attribute
//! buffers only for synchronous calls. Real handles transfer exactly once into
//! OwnedHandle/File; process handles are never files. LocalAlloc/FreeSid outputs
//! use their matching deallocator. No unsafe Send implementation is used.
#![allow(unsafe_code)]
use super::{commit::MAX_FILE, windows_policy};
use sha2::{Digest, Sha256};
use shixu_core::contracts::{AppResult, error::AppError};
use std::{
    ffi::c_void,
    fs::File,
    io::{Read, Write},
    mem::{size_of, zeroed},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    sync::Arc,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, Isolation::*, *},
    Storage::FileSystem::*,
    System::{
        IO::CancelSynchronousIo, JobObjects::*, Pipes::*, SystemServices::*, Threading::*,
        WindowsProgramming::DRIVE_FIXED,
    },
};
const MEMORY: usize = 512 * 1024 * 1024;
fn wide(value: &std::ffi::OsStr) -> AppResult<Vec<u16>> {
    let mut result: Vec<_> = value.encode_wide().collect();
    if result.contains(&0) {
        return Err(AppError::InvalidInput);
    }
    result.push(0);
    Ok(result)
}
fn path_wide(path: &Path) -> AppResult<Vec<u16>> {
    windows_policy::local_path(path.to_str().ok_or(AppError::Unsupported)?)?;
    wide(path.as_os_str())
}
fn os_error() -> AppError {
    // SAFETY: immediate thread-local error query after a failed Win32 call.
    match unsafe { GetLastError() } {
        ERROR_DISK_FULL | ERROR_HANDLE_DISK_FULL => AppError::StorageFull,
        ERROR_ALREADY_EXISTS | ERROR_FILE_EXISTS | ERROR_SHARING_VIOLATION => AppError::Conflict,
        _ => AppError::Unsupported,
    }
}
fn owned(raw: HANDLE) -> AppResult<OwnedHandle> {
    if raw.is_null() || raw == INVALID_HANDLE_VALUE {
        return Err(os_error());
    }
    // SAFETY: caller supplies a newly returned, uniquely owned real handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}
fn raw(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle()
}
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn token_info(token: &OwnedHandle, class: TOKEN_INFORMATION_CLASS) -> AppResult<Vec<usize>> {
    let mut length = 0;
    // SAFETY: documented size query and aligned owned buffer; token remains live.
    unsafe {
        GetTokenInformation(raw(token), class, null_mut(), 0, &mut length);
    }
    if length == 0 || length > 65536 {
        return Err(AppError::Unsupported);
    }
    let mut data = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            raw(token),
            class,
            data.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(os_error());
    }
    Ok(data)
}
fn process_token(process: HANDLE) -> AppResult<OwnedHandle> {
    let mut token = null_mut();
    // SAFETY: borrowed live process handle, initialized output uniquely owned.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(os_error());
    }
    owned(token)
}
fn sid_text(sid: PSID) -> AppResult<String> {
    let mut text = null_mut();
    // SAFETY: SID obtained from token/profile; allocated string released once.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(os_error());
    }
    let allocation = Local(text.cast());
    let mut len = 0;
    unsafe {
        while len < 256 && *text.add(len) != 0 {
            len += 1;
        }
    }
    if len == 256 {
        return Err(AppError::Unsupported);
    }
    let result = unsafe { String::from_utf16(std::slice::from_raw_parts(text, len)) }
        .map_err(|_| AppError::Unsupported);
    drop(allocation);
    result
}
#[cfg(test)]
fn current_sid() -> AppResult<String> {
    let token = process_token(unsafe { GetCurrentProcess() })?;
    let info = token_info(&token, TokenUser)?;
    sid_text(unsafe { (*info.as_ptr().cast::<TOKEN_USER>()).User.Sid })
}
pub(super) struct Profile {
    name: Vec<u16>,
    sid: Vec<u32>,
    created: bool,
}
impl Profile {
    #[cfg(test)]
    fn new(name: &str) -> AppResult<Self> {
        let name = wide(std::ffi::OsStr::new(name))?;
        let mut sid = null_mut();
        // SAFETY: fixed native profile name, zero capabilities; SID owned until drop.
        let result = unsafe {
            CreateAppContainerProfile(
                name.as_ptr(),
                name.as_ptr(),
                name.as_ptr(),
                null(),
                0,
                &mut sid,
            )
        };
        let created = result == 0;
        if !created {
            if result != 0x800700b7u32 as i32 {
                return Err(AppError::Unsupported);
            }
            if unsafe { DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &mut sid) } != 0 {
                return Err(AppError::Unsupported);
            }
        }
        if sid.is_null() {
            return Err(AppError::Unsupported);
        }
        let length = unsafe { GetLengthSid(sid) } as usize;
        if length == 0 || length > 256 {
            unsafe {
                FreeSid(sid);
            }
            return Err(AppError::Unsupported);
        }
        let mut owned_sid = vec![0u32; length.div_ceil(4)];
        unsafe {
            std::ptr::copy_nonoverlapping(
                sid.cast::<u8>(),
                owned_sid.as_mut_ptr().cast::<u8>(),
                length,
            );
            FreeSid(sid);
        }
        Ok(Self {
            name,
            sid: owned_sid,
            created,
        })
    }
}
impl Profile {
    fn sid(&self) -> PSID {
        self.sid.as_ptr().cast_mut().cast()
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        // SAFETY: owned profile name stays live; delete only our freshly created profile.
        unsafe {
            if self.created {
                DeleteAppContainerProfile(self.name.as_ptr());
            }
        }
    }
}
struct Attributes {
    data: Vec<usize>,
    initialized: bool,
}
impl Attributes {
    fn new() -> AppResult<Self> {
        let mut size = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut size);
        }
        if size == 0 || size > 65536 {
            return Err(AppError::Unsupported);
        }
        let mut result = Self {
            data: vec![0; size.div_ceil(size_of::<usize>())],
            initialized: false,
        };
        if unsafe { InitializeProcThreadAttributeList(result.ptr(), 2, 0, &mut size) } == 0 {
            return Err(os_error());
        }
        result.initialized = true;
        Ok(result)
    }
    fn ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.data.as_mut_ptr().cast()
    }
    fn set(&mut self, attribute: usize, value: *const c_void, size: usize) -> AppResult<()> {
        if unsafe {
            UpdateProcThreadAttribute(self.ptr(), 0, attribute, value, size, null_mut(), null())
        } == 0
        {
            return Err(os_error());
        }
        Ok(())
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.ptr());
            }
        }
    }
}
pub(super) struct WindowsProcess {
    process: OwnedHandle,
    job: Option<OwnedHandle>,
    _profile: Arc<Profile>,
}
impl WindowsProcess {
    pub(super) fn kill(&mut self) -> std::io::Result<()> {
        if unsafe {
            TerminateJobObject(
                raw(self
                    .job
                    .as_ref()
                    .ok_or_else(|| std::io::Error::other("Job closed"))?),
                1,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    pub(super) fn wait(&mut self) -> std::io::Result<()> {
        if unsafe { WaitForSingleObject(raw(&self.process), 2000) } != WAIT_OBJECT_0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "bounded helper termination failed",
            ));
        }
        Ok(())
    }
}
impl Drop for WindowsProcess {
    fn drop(&mut self) {
        let _ = self.kill();
        let _ = self.wait();
    }
}
fn pipes() -> AppResult<(OwnedHandle, OwnedHandle)> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let mut read = null_mut();
    let mut write = null_mut();
    if unsafe { CreatePipe(&mut read, &mut write, &attributes, 0) } == 0 {
        return Err(os_error());
    }
    Ok((owned(read)?, owned(write)?))
}
pub(super) struct Suspended {
    pub(super) process: WindowsProcess,
    pub(super) thread: OwnedHandle,
    pub(super) input: File,
    pub(super) output: File,
}
impl Suspended {
    pub(super) fn resume_thread(thread: &OwnedHandle) -> AppResult<()> {
        if unsafe { ResumeThread(raw(thread)) } == u32::MAX {
            return Err(os_error());
        }
        Ok(())
    }
}
fn verify_token(process: HANDLE, profile: &Profile) -> AppResult<()> {
    let token = process_token(process)?;
    let ac = token_info(&token, TokenIsAppContainer)?;
    let sid = token_info(&token, TokenAppContainerSid)?;
    let caps = token_info(&token, TokenCapabilities)?;
    let integrity = token_info(&token, TokenIntegrityLevel)?;
    unsafe {
        if *ac.as_ptr().cast::<u32>() != 1
            || EqualSid(
                (*sid.as_ptr().cast::<TOKEN_APPCONTAINER_INFORMATION>()).TokenAppContainer,
                profile.sid(),
            ) == 0
            || (*caps.as_ptr().cast::<TOKEN_GROUPS>()).GroupCount != 0
        {
            return Err(AppError::Unsupported);
        }
        let label = (*integrity.as_ptr().cast::<TOKEN_MANDATORY_LABEL>())
            .Label
            .Sid;
        let count = *GetSidSubAuthorityCount(label);
        if count == 0
            || *GetSidSubAuthority(label, u32::from(count - 1)) != SECURITY_MANDATORY_LOW_RID as u32
        {
            return Err(AppError::Unsupported);
        }
    }
    Ok(())
}
fn node_executable(resources: &Path) -> PathBuf {
    resources.join("runtime").join("node.exe")
}
fn helper_script(resources: &Path) -> PathBuf {
    resources.join("helper").join("helper.mjs")
}
#[cfg(test)]
fn fixture_resources(manifest: &Path) -> PathBuf {
    // Cargo's trusted absolute manifest directory may use either separator spelling.
    // Rebuild its native components, then traverse parents without introducing '..'.
    let manifest: PathBuf = manifest.components().collect();
    manifest
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("resources")
        .join("vault-win-x64")
}
pub(super) fn launch(
    resources: &Path,
    inbox: &Path,
    profile: Arc<Profile>,
) -> AppResult<Suspended> {
    let executable = node_executable(resources);
    let args = vec![
        executable.to_str().ok_or(AppError::Unsupported)?.to_owned(),
        "--permission".into(),
        "--no-addons".into(),
        "--no-warnings".into(),
        "--max-old-space-size=128".into(),
        format!("--allow-fs-read={}", resources.join("helper").display()),
        format!("--allow-fs-read={}", inbox.display()),
        format!("--allow-fs-write={}", inbox.display()),
        helper_script(resources)
            .to_str()
            .ok_or(AppError::Unsupported)?
            .to_owned(),
    ];
    launch_fixed(&executable, &args, inbox, profile)
}
fn launch_fixed(
    executable: &Path,
    args: &[String],
    inbox: &Path,
    profile: Arc<Profile>,
) -> AppResult<Suspended> {
    let executable = path_wide(executable)?;
    let cwd = path_wide(inbox)?;
    let command = args
        .iter()
        .map(|arg| windows_policy::quote(arg))
        .collect::<AppResult<Vec<_>>>()?
        .join(" ");
    let mut command = wide(std::ffi::OsStr::new(&command))?;
    // Empty environment block: no inherited secrets, PATH, NODE_OPTIONS or TEMP.
    let environment = [0u16, 0];
    let (child_input, parent_input) = pipes()?;
    let (parent_output, child_output) = pipes()?;
    for end in [&parent_input, &parent_output] {
        if unsafe { SetHandleInformation(raw(end), HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(os_error());
        }
    }
    let nul = wide(std::ffi::OsStr::new("NUL"))?;
    let inheritable = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let stderr = owned(unsafe {
        CreateFileW(
            nul.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &inheritable,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    })?;
    let handles = [raw(&child_input), raw(&child_output), raw(&stderr)];
    let capabilities = SECURITY_CAPABILITIES {
        AppContainerSid: profile.sid(),
        Capabilities: null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };
    let mut attributes = Attributes::new()?;
    attributes.set(
        PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
        (&capabilities as *const SECURITY_CAPABILITIES).cast(),
        size_of::<SECURITY_CAPABILITIES>(),
    )?;
    attributes.set(
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
        handles.as_ptr().cast(),
        size_of_val(&handles),
    )?;
    let mut startup: STARTUPINFOEXW = unsafe { zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = handles[0];
    startup.StartupInfo.hStdOutput = handles[1];
    startup.StartupInfo.hStdError = handles[2];
    startup.lpAttributeList = attributes.ptr();
    let job = owned(unsafe { CreateJobObjectW(null(), null()) })?;
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY
        | JOB_OBJECT_LIMIT_JOB_MEMORY;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    limits.ProcessMemoryLimit = MEMORY;
    limits.JobMemoryLimit = MEMORY;
    if unsafe {
        SetInformationJobObject(
            raw(&job),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of_val(&limits) as u32,
        )
    } == 0
    {
        return Err(os_error());
    }
    let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
        UIRestrictionsClass: JOB_OBJECT_UILIMIT_HANDLES
            | JOB_OBJECT_UILIMIT_READCLIPBOARD
            | JOB_OBJECT_UILIMIT_WRITECLIPBOARD
            | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
            | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
            | JOB_OBJECT_UILIMIT_GLOBALATOMS
            | JOB_OBJECT_UILIMIT_DESKTOP
            | JOB_OBJECT_UILIMIT_EXITWINDOWS,
    };
    if unsafe {
        SetInformationJobObject(
            raw(&job),
            JobObjectBasicUIRestrictions,
            (&ui as *const JOBOBJECT_BASIC_UI_RESTRICTIONS).cast(),
            size_of_val(&ui) as u32,
        )
    } == 0
    {
        return Err(os_error());
    }
    let mut info: PROCESS_INFORMATION = unsafe { zeroed() };
    // SAFETY: explicit executable and all backing buffers/attribute values live through call.
    if unsafe {
        CreateProcessW(
            executable.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT
                | CREATE_SUSPENDED
                | CREATE_NO_WINDOW
                | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            cwd.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        )
    } == 0
    {
        return Err(os_error());
    }
    let process = owned(info.hProcess)?;
    let thread = owned(info.hThread)?;
    let mut owner = WindowsProcess {
        process,
        job: Some(job),
        _profile: profile.clone(),
    };
    if unsafe {
        AssignProcessToJobObject(
            raw(owner.job.as_ref().ok_or(AppError::Unsupported)?),
            raw(&owner.process),
        )
    } == 0
    {
        // Before Job membership, terminate the suspended process directly.
        unsafe {
            TerminateProcess(raw(&owner.process), 1);
        }
        let _ = owner.wait();
        return Err(os_error());
    }
    let mut member = 0;
    let mut queried: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
    if unsafe {
        IsProcessInJob(
            raw(&owner.process),
            raw(owner.job.as_ref().ok_or(AppError::Unsupported)?),
            &mut member,
        )
    } == 0
        || member == 0
        || unsafe {
            QueryInformationJobObject(
                raw(owner.job.as_ref().ok_or(AppError::Unsupported)?),
                JobObjectExtendedLimitInformation,
                (&mut queried as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of_val(&queried) as u32,
                null_mut(),
            )
        } == 0
        || queried.BasicLimitInformation.LimitFlags != limits.BasicLimitInformation.LimitFlags
        || queried.BasicLimitInformation.ActiveProcessLimit != 1
        || queried.ProcessMemoryLimit != MEMORY
        || queried.JobMemoryLimit != MEMORY
    {
        return Err(AppError::Unsupported);
    }
    verify_token(raw(&owner.process), &profile)?;
    drop(child_input);
    drop(child_output);
    drop(stderr);
    Ok(Suspended {
        process: owner,
        thread,
        input: File::from(parent_input),
        output: File::from(parent_output),
    })
}
#[derive(Clone, Copy)]
enum Access {
    Private,
    Inbox,
    Resources,
    Traverse,
}
fn descriptor(owner: &str, profile: &str, access: Access, directory: bool) -> AppResult<Local> {
    let inherit = if directory { "OICI" } else { "" };
    let grant = match access {
        Access::Private => String::new(),
        Access::Inbox => format!("(A;{inherit};0x001201bf;;;{profile})"),
        Access::Resources => format!("(A;{inherit};FRFX;;;{profile})"),
        Access::Traverse => format!("(A;;0x00000020;;;{profile})"),
    };
    let label = if matches!(access, Access::Inbox) {
        "LW"
    } else {
        "ME"
    };
    let policy = if matches!(access, Access::Private) {
        "NWNR"
    } else {
        "NW"
    };
    let sddl =
        format!("O:{owner}D:P(A;{inherit};FA;;;{owner}){grant}S:(ML;{inherit};{policy};;;{label})");
    let wide = wide(std::ffi::OsStr::new(&sddl))?;
    let mut raw = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(wide.as_ptr(), 1, &mut raw, null_mut())
    } == 0
    {
        return Err(os_error());
    }
    Ok(Local(raw))
}
#[cfg(test)]
fn create_directory(path: &Path, security: &Local) -> AppResult<()> {
    let path = path_wide(path)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    if unsafe { CreateDirectoryW(path.as_ptr(), &attributes) } == 0 {
        return Err(os_error());
    }
    Ok(())
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity {
    volume: u32,
    high: u32,
    low: u32,
}
struct Admission {
    file: File,
    identity: Identity,
}
fn open_handle(path: &Path, directory: bool, write: bool, sharing: u32) -> AppResult<Admission> {
    let path16 = path_wide(path)?;
    let handle = owned(unsafe {
        CreateFileW(
            path16.as_ptr(),
            FILE_READ_ATTRIBUTES
                | READ_CONTROL
                | GENERIC_READ
                | if write { GENERIC_WRITE } else { 0 },
            sharing,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
            null_mut(),
        )
    })?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    if unsafe { GetFileInformationByHandle(raw(&handle), &mut info) } == 0
        || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || (!directory
            && (info.nNumberOfLinks != 1
                || info.nFileSizeHigh != 0
                || u64::from(info.nFileSizeLow) > 256 * 1024 * 1024))
    {
        return Err(AppError::Unsupported);
    }
    let drive: Vec<u16> = path16[..3].iter().copied().chain(Some(0)).collect();
    if unsafe { GetDriveTypeW(drive.as_ptr()) } != DRIVE_FIXED {
        return Err(AppError::Unsupported);
    }
    let mut final_path = vec![0u16; 32768];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            raw(&handle),
            final_path.as_mut_ptr(),
            final_path.len() as u32,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    } as usize;
    if length == 0 || length >= final_path.len() {
        return Err(AppError::Unsupported);
    }
    let final_path =
        String::from_utf16(&final_path[..length]).map_err(|_| AppError::Unsupported)?;
    if final_path
        .strip_prefix("\\\\?\\")
        .ok_or(AppError::Unsupported)?
        .to_lowercase()
        != path.to_str().ok_or(AppError::Unsupported)?.to_lowercase()
    {
        return Err(AppError::Unsupported);
    }
    if !directory {
        let mut stream: WIN32_FIND_STREAM_DATA = unsafe { zeroed() };
        let search = unsafe {
            FindFirstStreamW(
                path16.as_ptr(),
                FindStreamInfoStandard,
                (&mut stream as *mut WIN32_FIND_STREAM_DATA).cast(),
                0,
            )
        };
        if search == INVALID_HANDLE_VALUE {
            return Err(os_error());
        }
        let default: Vec<_> = "::$DATA".encode_utf16().chain(Some(0)).collect();
        let valid = stream.cStreamName[..default.len()] == default
            && unsafe {
                FindNextStreamW(search, (&mut stream as *mut WIN32_FIND_STREAM_DATA).cast())
            } == 0
            && unsafe { GetLastError() } == ERROR_HANDLE_EOF;
        unsafe {
            FindClose(search);
        }
        if !valid {
            return Err(AppError::Unsupported);
        }
    }
    Ok(Admission {
        file: File::from(handle),
        identity: Identity {
            volume: info.dwVolumeSerialNumber,
            high: info.nFileIndexHigh,
            low: info.nFileIndexLow,
        },
    })
}
fn ancestors(path: &Path) -> AppResult<Vec<Admission>> {
    let mut paths: Vec<_> = path.ancestors().collect();
    paths.reverse();
    paths
        .into_iter()
        .map(|part| open_handle(part, true, false, FILE_SHARE_READ | FILE_SHARE_WRITE))
        .collect()
}
fn validate_security(
    file: &File,
    owner: &str,
    profile: &str,
    access: Access,
    directory: bool,
) -> AppResult<()> {
    let mut actual = null_mut();
    let mut actual_owner = null_mut();
    let mut dacl = null_mut();
    let mut sacl = null_mut();
    // LABEL_SECURITY_INFORMATION reads MIC without requiring SeSecurityPrivilege.
    if unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
            &mut actual_owner,
            null_mut(),
            &mut dacl,
            &mut sacl,
            &mut actual,
        )
    } != 0
    {
        return Err(AppError::Unsupported);
    }
    let allocation = Local(actual);
    let mut control = 0;
    let mut revision = 0;
    if unsafe { GetSecurityDescriptorControl(actual, &mut control, &mut revision) } == 0
        || (control & SE_DACL_PROTECTED == 0 && !matches!(access, Access::Inbox))
        || dacl.is_null()
        || sacl.is_null()
        || sid_text(actual_owner)? != owner
    {
        return Err(AppError::Unsupported);
    }
    // Exact allow-ACE policy; no broad SID, ownership or DACL grant to helper.
    let count = unsafe { (*dacl).AceCount };
    let expected_count = if matches!(access, Access::Private) {
        1
    } else {
        2
    };
    if count != expected_count {
        return Err(AppError::Unsupported);
    }
    let mut seen = std::collections::BTreeSet::new();
    for index in 0..u32::from(count) {
        let mut ace = null_mut();
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
            return Err(AppError::Unsupported);
        }
        // Reject foreign layouts before reading mask/SID fields from that ACE.
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        if header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8 || header.AceSize < 16 {
            return Err(AppError::Unsupported);
        }
        let entry = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        let sid = sid_text((&entry.SidStart as *const u32).cast_mut().cast())?;
        if !seen.insert(sid.clone()) {
            return Err(AppError::Unsupported);
        }
        let expected_mask = if sid == owner {
            FILE_ALL_ACCESS
        } else if sid == profile {
            match access {
                Access::Inbox => 0x001201bf,
                Access::Resources => FILE_GENERIC_READ | FILE_GENERIC_EXECUTE,
                Access::Traverse => FILE_TRAVERSE,
                Access::Private => return Err(AppError::Unsupported),
            }
        } else {
            return Err(AppError::Unsupported);
        };
        let expected_flags = if directory && (sid == owner || !matches!(access, Access::Traverse)) {
            OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
        } else {
            0
        };
        // Files created by helper can carry INHERITED_ACE, but no additional rights.
        if entry.Header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8
            || entry.Mask != expected_mask
            || entry.Header.AceFlags & !(INHERITED_ACE as u8) != expected_flags as u8
        {
            return Err(AppError::Unsupported);
        }
    }
    if unsafe { (*sacl).AceCount } != 1 {
        return Err(AppError::Unsupported);
    }
    let mut label = null_mut();
    if unsafe { GetAce(sacl, 0, &mut label) } == 0 {
        return Err(AppError::Unsupported);
    }
    let header = unsafe { &*label.cast::<ACE_HEADER>() };
    if header.AceType != SYSTEM_MANDATORY_LABEL_ACE_TYPE as u8 || header.AceSize < 16 {
        return Err(AppError::Unsupported);
    }
    let label = unsafe { &*label.cast::<SYSTEM_MANDATORY_LABEL_ACE>() };
    let level = sid_text((&label.SidStart as *const u32).cast_mut().cast())?;
    let expected_level = if matches!(access, Access::Inbox) {
        "S-1-16-4096"
    } else {
        "S-1-16-8192"
    };
    let mask = SYSTEM_MANDATORY_LABEL_NO_WRITE_UP
        | if matches!(access, Access::Private) {
            SYSTEM_MANDATORY_LABEL_NO_READ_UP
        } else {
            0
        };
    let flags = if directory {
        OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
    } else {
        0
    };
    if label.Header.AceFlags & !(INHERITED_ACE as u8) != flags as u8
        || label.Header.AceType != SYSTEM_MANDATORY_LABEL_ACE_TYPE as u8
        || label.Mask != mask
        || level != expected_level
    {
        return Err(AppError::Unsupported);
    }
    drop(allocation);
    Ok(())
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn read(admission: &mut Admission, maximum: u64) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    Read::by_ref(&mut admission.file)
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| AppError::AuthFailed)?;
    if bytes.len() as u64 > maximum {
        return Err(AppError::Unsupported);
    }
    Ok(bytes)
}
fn create_file(path: &Path, security: &Local, bytes: &[u8]) -> AppResult<()> {
    let path = path_wide(path)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    let handle = owned(unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE | READ_CONTROL,
            FILE_SHARE_READ,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    })?;
    let mut file = File::from(handle);
    file.write_all(bytes).map_err(|e| {
        if e.kind() == std::io::ErrorKind::StorageFull {
            AppError::StorageFull
        } else {
            AppError::AuthFailed
        }
    })?;
    if unsafe { FlushFileBuffers(file.as_raw_handle()) } == 0 {
        return Err(os_error());
    }
    Ok(())
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Failure {
    None,
    BeforeCheckpoint,
    AfterCheckpoint,
    BeforePending,
    AfterPending,
    BeforeReplace,
    AfterReplace,
    BeforeFinalFlush,
    AfterFinalFlush,
}
pub(super) struct Store {
    root: PathBuf,
    pub(super) inbox: PathBuf,
    pub(super) resources: PathBuf,
    pub(super) profile: Arc<Profile>,
    owner: String,
    profile_sid: String,
    _pins: Vec<Admission>,
    _lease: Admission,
    expected_identity: Option<Identity>,
    pub(super) failure: Failure,
}
impl Store {
    fn admit(&self, path: &Path, access: Access, write: bool) -> AppResult<Admission> {
        let file = open_handle(path, false, write, FILE_SHARE_READ)?;
        validate_security(&file.file, &self.owner, &self.profile_sid, access, false)?;
        Ok(file)
    }
    pub(super) fn digest_active(&mut self) -> AppResult<String> {
        let mut file = self.admit(&self.root.join("vault.kdbx"), Access::Private, false)?;
        let digest = hash(&read(&mut file, MAX_FILE)?);
        if self
            .expected_identity
            .is_some_and(|expected| expected != file.identity)
        {
            return Err(AppError::Conflict);
        }
        self.expected_identity = Some(file.identity);
        Ok(digest)
    }
    pub(super) fn active_exists(&self) -> AppResult<bool> {
        self.root
            .join("vault.kdbx")
            .try_exists()
            .map_err(|_| AppError::Unsupported)
    }
    pub(super) fn verify(&self) -> AppResult<()> {
        let _ancestors = ancestors(&self.resources)?;
        let root = open_handle(
            &self.resources,
            true,
            false,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )?;
        validate_security(
            &root.file,
            &self.owner,
            &self.profile_sid,
            Access::Resources,
            true,
        )?;
        let base = self.root.parent().ok_or(AppError::Unsupported)?;
        let base = open_handle(base, true, false, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
        validate_security(
            &base.file,
            &self.owner,
            &self.profile_sid,
            Access::Traverse,
            true,
        )?;
        super::engine::verify_resources(&self.resources)
    }
    pub(super) fn snapshot(&mut self) -> AppResult<()> {
        self.verify()?;
        self.expected_identity = None;
        let stale = self.root.join("vault.pending.kdbx");
        if stale.try_exists().map_err(|_| AppError::Unsupported)? {
            self.admit(&stale, Access::Private, false)?;
            std::fs::remove_file(stale).map_err(|_| AppError::Unsupported)?;
        }
        for name in ["vault.kdbx", "vault.pending.kdbx"] {
            let path = self.inbox.join(name);
            if path.try_exists().map_err(|_| AppError::Unsupported)? {
                self.admit(&path, Access::Inbox, false)?;
                std::fs::remove_file(path).map_err(|_| AppError::Unsupported)?;
            }
        }
        if self.active_exists()? {
            let mut active = self.admit(&self.root.join("vault.kdbx"), Access::Private, false)?;
            self.expected_identity = Some(active.identity);
            create_file(
                &self.inbox.join("vault.kdbx"),
                &descriptor(&self.owner, &self.profile_sid, Access::Inbox, false)?,
                &read(&mut active, MAX_FILE)?,
            )?;
        }
        Ok(())
    }
    fn fail(&self, point: Failure) -> AppResult<()> {
        if self.failure != Failure::None && self.failure == point {
            Err(AppError::AuthFailed)
        } else {
            Ok(())
        }
    }
    pub(super) fn commit(&mut self, expected: Option<&str>, verified: &str) -> AppResult<String> {
        let mut staged =
            self.admit(&self.inbox.join("vault.pending.kdbx"), Access::Inbox, false)?;
        let bytes = read(&mut staged, MAX_FILE)?;
        let next = hash(&bytes);
        if next != verified {
            return Err(AppError::AuthFailed);
        }
        let active = self.root.join("vault.kdbx");
        let pending = self.root.join("vault.pending.kdbx");
        let old = if let Some(expected) = expected {
            if self.digest_active()? != expected {
                return Err(AppError::Conflict);
            }
            let mut current = self.admit(&active, Access::Private, false)?;
            Some(read(&mut current, MAX_FILE)?)
        } else {
            if self.active_exists()? {
                return Err(AppError::Conflict);
            }
            None
        };
        let security = descriptor(&self.owner, &self.profile_sid, Access::Private, false)?;
        self.fail(Failure::BeforeCheckpoint)?;
        if let Some(old) = old {
            // New immutable checkpoint before replacement; never erase the only old copy.
            let checkpoint = self
                .root
                .join(format!("vault.checkpoint-{}.kdbx", uuid::Uuid::new_v4()));
            create_file(&checkpoint, &security, &old)?;
            self.admit(&checkpoint, Access::Private, false)?;
        }
        self.fail(Failure::AfterCheckpoint)?;
        self.fail(Failure::BeforePending)?;
        create_file(&pending, &security, &bytes)?;
        self.fail(Failure::AfterPending)?;
        let mut admitted = self.admit(&pending, Access::Private, true)?;
        if admitted.identity.volume != self._lease.identity.volume
            || hash(&read(&mut admitted, MAX_FILE)?) != next
        {
            return Err(AppError::Conflict);
        }
        drop(admitted);
        if let Some(expected) = expected
            && self.digest_active()? != expected
        {
            return Err(AppError::Conflict);
        }
        self.fail(Failure::BeforeReplace)?;
        let active16 = path_wide(&active)?;
        let pending16 = path_wide(&pending)?;
        // ReplaceFile is pathname based: cooperating lease, not hostile same-user CAS.
        let success = unsafe {
            if expected.is_some() {
                ReplaceFileW(
                    active16.as_ptr(),
                    pending16.as_ptr(),
                    null(),
                    0,
                    null(),
                    null(),
                )
            } else {
                MoveFileExW(
                    pending16.as_ptr(),
                    active16.as_ptr(),
                    MOVEFILE_WRITE_THROUGH,
                )
            }
        };
        if success == 0 {
            return Err(os_error());
        }
        self.fail(Failure::AfterReplace)?;
        let mut final_file = self.admit(&active, Access::Private, true)?;
        if final_file.identity.volume != self._lease.identity.volume
            || hash(&read(&mut final_file, MAX_FILE)?) != next
        {
            return Err(AppError::Conflict);
        }
        self.fail(Failure::BeforeFinalFlush)?;
        if unsafe { FlushFileBuffers(final_file.file.as_raw_handle()) } == 0 {
            return Err(os_error());
        }
        self.fail(Failure::AfterFinalFlush)?;
        self.expected_identity = Some(final_file.identity);
        drop(final_file);
        drop(staged);
        self.snapshot()?;
        self.digest_active()?;
        Ok(next)
    }
}
pub(super) fn finish_worker(worker: std::thread::JoinHandle<()>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !worker.is_finished() && std::time::Instant::now() < deadline {
        // Thread handle is borrowed from JoinHandle, never closed or transferred.
        unsafe {
            CancelSynchronousIo(worker.as_raw_handle());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if worker.is_finished() {
        let _ = worker.join();
    }
    // A failed kernel termination must not cause an unbounded join. Detaching
    // is exceptional cleanup, never save success, and requires native evidence.
}
#[cfg(test)]
impl Store {
    fn synthetic(base: &Path, source: &Path) -> AppResult<Self> {
        let owner = current_sid()?;
        let profile = Arc::new(Profile::new(&format!(
            "shixu.synthetic.{}",
            uuid::Uuid::new_v4()
        ))?);
        let profile_sid = sid_text(profile.sid())?;
        let mut pins = ancestors(base.parent().ok_or(AppError::Unsupported)?)?;
        create_directory(
            base,
            &descriptor(&owner, &profile_sid, Access::Traverse, true)?,
        )?;
        let base_guard = open_handle(base, true, false, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
        validate_security(
            &base_guard.file,
            &owner,
            &profile_sid,
            Access::Traverse,
            true,
        )?;
        pins.push(base_guard);
        let root = base.join("active");
        let inbox = base.join("inbox");
        let resources = base.join("resources");
        for (path, access) in [
            (&root, Access::Private),
            (&inbox, Access::Inbox),
            (&resources, Access::Resources),
        ] {
            create_directory(path, &descriptor(&owner, &profile_sid, access, true)?)?;
            let guard = open_handle(path, true, false, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
            validate_security(&guard.file, &owner, &profile_sid, access, true)?;
            pins.push(guard);
        }
        fn copy(
            source: &Path,
            target: &Path,
            owner: &str,
            profile: &str,
            pins: &mut Vec<Admission>,
        ) -> AppResult<()> {
            for entry in std::fs::read_dir(source).map_err(|_| AppError::Unsupported)? {
                let entry = entry.map_err(|_| AppError::Unsupported)?;
                let path = entry.path();
                let target = target.join(entry.file_name());
                let meta = std::fs::symlink_metadata(&path).map_err(|_| AppError::Unsupported)?;
                if meta.is_dir() {
                    let _source_guard =
                        open_handle(&path, true, false, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
                    create_directory(
                        &target,
                        &descriptor(owner, profile, Access::Resources, true)?,
                    )?;
                    let guard =
                        open_handle(&target, true, false, FILE_SHARE_READ | FILE_SHARE_WRITE)?;
                    validate_security(&guard.file, owner, profile, Access::Resources, true)?;
                    pins.push(guard);
                    copy(&path, &target, owner, profile, pins)?;
                } else {
                    let mut guard = open_handle(&path, false, false, FILE_SHARE_READ)?;
                    create_file(
                        &target,
                        &descriptor(owner, profile, Access::Resources, false)?,
                        &read(&mut guard, 256 * 1024 * 1024)?,
                    )?;
                    let guard = open_handle(&target, false, false, FILE_SHARE_READ)?;
                    validate_security(&guard.file, owner, profile, Access::Resources, false)?;
                    pins.push(guard);
                }
            }
            Ok(())
        }
        let _source_ancestors = ancestors(source)?;
        super::engine::verify_resources(source)?;
        copy(source, &resources, &owner, &profile_sid, &mut pins)?;
        super::engine::verify_resources(&resources)?;
        let lease_path = root.join("vault.lease");
        create_file(
            &lease_path,
            &descriptor(&owner, &profile_sid, Access::Private, false)?,
            b"synthetic lease",
        )?;
        let lease = open_handle(&lease_path, false, true, 0)?;
        validate_security(&lease.file, &owner, &profile_sid, Access::Private, false)?;
        Ok(Self {
            root,
            inbox,
            resources,
            profile,
            owner,
            profile_sid,
            _pins: pins,
            _lease: lease,
            expected_identity: None,
            failure: Failure::None,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::super::{
        engine::KdbxWebEngine,
        pipe::{PrivatePipe, VaultCancellation},
    };
    use super::*;
    use shixu_core::{
        contracts::vault::{SecretBytes, VaultMutation},
        vault::ports::VaultEngine,
    };
    use std::{
        net::{TcpListener, TcpStream, UdpSocket},
        time::{Duration, Instant},
    };
    fn secret(value: &str) -> SecretBytes {
        SecretBytes::new(value.as_bytes().to_vec())
    }
    fn fixture() -> (Store, PathBuf) {
        // No env override, arbitrary executable selection, production root or secrets.
        let base =
            std::env::temp_dir().join(format!("shixu synthetic 空 格 {}", uuid::Uuid::new_v4()));
        let store = Store::synthetic(
            &base,
            &fixture_resources(Path::new(env!("CARGO_MANIFEST_DIR"))),
        )
        .expect("BLOCKED: fixed Windows resource/ACL fixture admission failed");
        (store, base)
    }
    #[test]
    fn actual_trusted_constructors_pass_strict_windows_admission() {
        let actual = fixture_resources(Path::new(env!("CARGO_MANIFEST_DIR")));
        assert!(path_wide(&actual).is_ok());
        assert!(path_wide(&node_executable(&actual)).is_ok());
        assert!(path_wide(&helper_script(&actual)).is_ok());
        for manifest in [
            r"C:\src\空 格\repo\crates\shixu-native",
            "C:/src/空 格/repo/crates/shixu-native",
        ] {
            let resources = fixture_resources(Path::new(manifest));
            assert_eq!(
                resources,
                PathBuf::from(r"C:\src\空 格\repo\resources\vault-win-x64")
            );
            assert!(path_wide(&resources).is_ok());
            assert!(path_wide(&node_executable(&resources)).is_ok());
            assert!(path_wide(&helper_script(&resources)).is_ok());
        }
        // The same validator still rejects slash-containing external paths.
        assert!(path_wide(Path::new("C:/untrusted/runtime/node.exe")).is_err());
    }
    #[test]
    fn production_constructor_remains_blocked() {
        assert!(matches!(
            KdbxWebEngine::prepared(
                PathBuf::from("C:\\nonexistent-resources"),
                PathBuf::from("C:\\nonexistent-vault")
            ),
            Err(AppError::Unsupported)
        ));
    }
    #[test]
    #[ignore = "Actual Windows only; invoked solely by fixed synthetic parent"]
    fn synthetic_os_diagnostic_child() {
        // The only diagnostic entry point is this test-only fixed action sequence.
        let inbox = std::env::current_dir().expect("fixed inbox");
        let base = inbox.parent().expect("fixed fixture parent");
        let metadata: serde_json::Value = serde_json::from_slice(
            &std::fs::read(inbox.join("probe.json")).expect("fixed probe metadata"),
        )
        .expect("fixed metadata parse");
        for path in [
            base.join("active").join("vault.kdbx"),
            base.join("active").join("vault.checkpoint-probe.kdbx"),
            base.join("calendar.marker"),
        ] {
            assert!(File::open(&path).is_err(), "OS private read must be denied");
            assert!(
                std::fs::OpenOptions::new().write(true).open(&path).is_err(),
                "OS private write must be denied"
            );
            assert!(
                std::fs::remove_file(&path).is_err(),
                "OS private delete must be denied"
            );
        }
        for entry in std::fs::read_dir(base.join("active"))
            .into_iter()
            .flatten()
            .flatten()
        {
            assert!(
                File::open(entry.path()).is_err(),
                "OS checkpoint read must be denied"
            );
        }
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(base.join("resources").join("helper").join("helper.mjs"))
                .is_err(),
            "OS resource write must be denied"
        );
        assert!(
            File::open(base.join("resources").join("helper").join("helper.mjs")).is_ok(),
            "OS resource read allowed"
        );
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(base.join("diagnostic").join("readonly.marker"))
                .is_err(),
            "OS resource DACL denies write without sharing guard"
        );
        // Verify semantic file identity if the inherited handle number collides.
        let handle = metadata["sentinel_handle"]
            .as_u64()
            .expect("handle metadata") as usize as HANDLE;
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
        if unsafe { GetFileInformationByHandle(handle, &mut info) } != 0 {
            assert!(
                u64::from(info.dwVolumeSerialNumber) != metadata["volume"].as_u64().unwrap()
                    || u64::from(info.nFileIndexHigh) != metadata["high"].as_u64().unwrap()
                    || u64::from(info.nFileIndexLow) != metadata["low"].as_u64().unwrap(),
                "sentinel file handle inherited"
            );
        }
        let tcp: std::net::SocketAddr = format!("127.0.0.1:{}", metadata["tcp"].as_u64().unwrap())
            .parse()
            .unwrap();
        assert!(
            TcpStream::connect_timeout(&tcp, Duration::from_secs(2)).is_err(),
            "OS loopback TCP denial missing"
        );
        let udp = UdpSocket::bind("127.0.0.1:0");
        if let Ok(udp) = udp {
            let _ = udp.send_to(
                b"synthetic-only",
                format!("127.0.0.1:{}", metadata["udp"].as_u64().unwrap()),
            );
        }
        let mut second = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .spawn();
        if let Ok(child) = &mut second {
            let _ = child.kill();
            let _ = child.wait();
        }
        assert!(second.is_err(), "one-process Job must deny second process");
        // Real committed allocation request; Job cap must reject it, not Node flags.
        use windows_sys::Win32::System::Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
        };
        let allocation = unsafe {
            VirtualAlloc(
                null(),
                MEMORY + 128 * 1024 * 1024,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        if !allocation.is_null() {
            unsafe {
                VirtualFree(allocation, 0, MEM_RELEASE);
            }
        }
        assert!(allocation.is_null(), "actual Job memory cap missing");
        std::fs::write(inbox.join("probe-done"), b"fixed-native-os-8")
            .expect("OS inbox write allowed");
    }
    fn diagnostic(store: &mut Store, base: &Path) {
        let diagnostic_root = base.join("diagnostic");
        create_directory(
            &diagnostic_root,
            &descriptor(&store.owner, &store.profile_sid, Access::Resources, true).unwrap(),
        )
        .unwrap();
        let program = diagnostic_root.join("fixed-test.exe");
        let current = std::env::current_exe().unwrap();
        let mut source = open_handle(&current, false, false, FILE_SHARE_READ).unwrap();
        let bytes = read(&mut source, 256 * 1024 * 1024).unwrap();
        let source_hash = hash(&bytes);
        create_file(
            &program,
            &descriptor(&store.owner, &store.profile_sid, Access::Resources, false).unwrap(),
            &bytes,
        )
        .unwrap();
        let mut guard = open_handle(&program, false, false, FILE_SHARE_READ).unwrap();
        validate_security(
            &guard.file,
            &store.owner,
            &store.profile_sid,
            Access::Resources,
            false,
        )
        .unwrap();
        assert_eq!(
            hash(&read(&mut guard, 256 * 1024 * 1024).unwrap()),
            source_hash
        );
        // Parent positive control rules out global commit exhaustion as the
        // cause of the contained allocation rejection.
        use windows_sys::Win32::System::Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
        };
        let control_allocation = unsafe {
            VirtualAlloc(
                null(),
                MEMORY + 128 * 1024 * 1024,
                MEM_RESERVE | MEM_COMMIT,
                PAGE_READWRITE,
            )
        };
        assert!(
            !control_allocation.is_null(),
            "BLOCKED: parent memory positive control unavailable"
        );
        assert_ne!(
            unsafe { VirtualFree(control_allocation, 0, MEM_RELEASE) },
            0
        );
        create_file(
            &diagnostic_root.join("readonly.marker"),
            &descriptor(&store.owner, &store.profile_sid, Access::Resources, false).unwrap(),
            b"readonly synthetic resource",
        )
        .unwrap();
        let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
        let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp.set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        // Live same-user positive controls precede contained attempts.
        let control = TcpStream::connect(tcp.local_addr().unwrap()).unwrap();
        let accepted = tcp.accept().unwrap();
        drop(control);
        drop(accepted);
        let udp_control = UdpSocket::bind("127.0.0.1:0").unwrap();
        udp_control
            .send_to(b"control", udp.local_addr().unwrap())
            .unwrap();
        let mut packet = [0; 64];
        assert_eq!(udp.recv(&mut packet).unwrap(), 7);
        let active = std::fs::read(store.root.join("vault.kdbx")).unwrap();
        create_file(
            &store.root.join("vault.checkpoint-probe.kdbx"),
            &descriptor(&store.owner, &store.profile_sid, Access::Private, false).unwrap(),
            &active,
        )
        .unwrap();
        let marker = base.join("calendar.marker");
        create_file(
            &marker,
            &descriptor(&store.owner, &store.profile_sid, Access::Private, false).unwrap(),
            b"synthetic-calendar-marker",
        )
        .unwrap();
        let sentinel = open_handle(&marker, false, false, FILE_SHARE_READ).unwrap();
        assert_ne!(
            unsafe {
                SetHandleInformation(
                    sentinel.file.as_raw_handle(),
                    HANDLE_FLAG_INHERIT,
                    HANDLE_FLAG_INHERIT,
                )
            },
            0
        );
        let metadata = serde_json::to_vec(&serde_json::json!({"tcp":tcp.local_addr().unwrap().port(),"udp":udp.local_addr().unwrap().port(),"sentinel_handle":sentinel.file.as_raw_handle() as usize,"volume":sentinel.identity.volume,"high":sentinel.identity.high,"low":sentinel.identity.low})).unwrap();
        create_file(
            &store.inbox.join("probe.json"),
            &descriptor(&store.owner, &store.profile_sid, Access::Inbox, false).unwrap(),
            &metadata,
        )
        .unwrap();
        let args = vec![
            program.to_str().unwrap().into(),
            "vault::windows::tests::synthetic_os_diagnostic_child".into(),
            "--exact".into(),
            "--ignored".into(),
            "--test-threads=1".into(),
        ];
        let child = launch_fixed(&program, &args, &store.inbox, store.profile.clone())
            .expect("BLOCKED: diagnostic launch");
        Suspended::resume_thread(&child.thread).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while unsafe { WaitForSingleObject(raw(&child.process.process), 10) } == WAIT_TIMEOUT
            && Instant::now() < deadline
        {}
        let mut code = 999;
        assert_ne!(
            unsafe { GetExitCodeProcess(raw(&child.process.process), &mut code) },
            0
        );
        assert_eq!(code, 0, "fixed OS diagnostic failed");
        assert_eq!(
            std::fs::read(store.inbox.join("probe-done")).unwrap(),
            b"fixed-native-os-8"
        );
        assert!(
            udp.recv(&mut packet).is_err(),
            "OS UDP packet reached parent"
        );
        drop(child.input);
        let mut output = Vec::new();
        child
            .output
            .take(1024 * 1024)
            .read_to_end(&mut output)
            .unwrap();
        // Never emit raw diagnostic stdout; only check fixed successful test count.
        assert!(String::from_utf8_lossy(&output).contains("1 passed; 0 failed"));
        assert_eq!(std::fs::read(marker).unwrap(), b"synthetic-calendar-marker");
    }
    fn resource_failures(base: &Path, store: &Store) {
        fn copy(source: &Path, target: &Path) {
            std::fs::create_dir(target).unwrap();
            for item in std::fs::read_dir(source).unwrap() {
                let item = item.unwrap();
                let dest = target.join(item.file_name());
                if item.file_type().unwrap().is_dir() {
                    copy(&item.path(), &dest);
                } else {
                    std::fs::copy(item.path(), dest).unwrap();
                }
            }
        }
        let target = base.join("resource-policy-fixture");
        copy(&store.resources, &target);
        super::super::engine::verify_resources(&target).unwrap();
        std::fs::write(target.join("extra"), b"extra").unwrap();
        assert!(super::super::engine::verify_resources(&target).is_err());
        std::fs::remove_file(target.join("extra")).unwrap();
        let helper = target.join("helper").join("helper.mjs");
        let bytes = std::fs::read(&helper).unwrap();
        std::fs::write(&helper, b"tamper").unwrap();
        assert!(super::super::engine::verify_resources(&target).is_err());
        std::fs::remove_file(&helper).unwrap();
        assert!(super::super::engine::verify_resources(&target).is_err());
        std::fs::write(&helper, bytes).unwrap();
        assert!(
            std::fs::OpenOptions::new()
                .write(true)
                .open(store.resources.join("helper").join("helper.mjs"))
                .is_err(),
            "pinned resources deny write sharing"
        );
        let broad = store.root.join("broad.kdbx");
        let sddl = wide(std::ffi::OsStr::new(&format!(
            "O:{}D:(A;;FA;;;WD)S:(ML;;NW;;;ME)",
            store.owner
        )))
        .unwrap();
        let mut sd = null_mut();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut sd,
                    null_mut(),
                )
            },
            0
        );
        create_file(&broad, &Local(sd), b"broad").unwrap();
        assert!(store.admit(&broad, Access::Private, false).is_err());
        assert!(
            open_handle(&store.root.join("vault.lease"), false, true, 0).is_err(),
            "single cooperating lease required"
        );
        assert!(
            open_handle(
                Path::new("\\\\localhost\\C$\\vault"),
                true,
                false,
                FILE_SHARE_READ
            )
            .is_err()
        );
    }
    fn checkpoint_decrypt(store: &Store, ciphertext: &[u8]) {
        let path = store.inbox.join("vault.kdbx");
        store.admit(&path, Access::Inbox, false).unwrap();
        std::fs::remove_file(&path).unwrap();
        create_file(
            &path,
            &descriptor(&store.owner, &store.profile_sid, Access::Inbox, false).unwrap(),
            ciphertext,
        )
        .unwrap();
        let mut pipe = PrivatePipe::spawn_windows(store, VaultCancellation::default()).unwrap();
        let reply = pipe
            .send(zeroize::Zeroizing::new(
                br#"{"v":1,"id":1,"op":"open","master":"synthetic-only-master-v2"}"#.to_vec(),
            ))
            .unwrap();
        let reply: serde_json::Value = serde_json::from_slice(&reply).unwrap();
        assert_eq!(reply["type"], "unit");
    }
    fn job_close_and_blocked_io(store: &Store) {
        let late = launch(&store.resources, &store.inbox, store.profile.clone()).unwrap();
        let observed = late.process.process.try_clone().unwrap();
        let cancelled = VaultCancellation::default();
        cancelled.cancel();
        assert!(matches!(
            PrivatePipe::register_windows(late, cancelled),
            Err(AppError::Locked)
        ));
        assert_eq!(
            unsafe { WaitForSingleObject(raw(&observed), 2000) },
            WAIT_OBJECT_0,
            "late cancelled registration executes no helper"
        );
        let mut child = launch(&store.resources, &store.inbox, store.profile.clone()).unwrap();
        Suspended::resume_thread(&child.thread).unwrap();
        let output = child.output;
        let worker = std::thread::spawn(move || {
            let mut output = output;
            let mut bytes = [0; 4];
            let _ = output.read_exact(&mut bytes);
        });
        let start = Instant::now();
        child.process.job.take(); // Last real Job handle closes here, independently of kill().
        assert_eq!(
            unsafe { WaitForSingleObject(raw(&child.process.process), 2000) },
            WAIT_OBJECT_0,
            "Job close must terminate helper"
        );
        finish_worker(worker);
        assert!(start.elapsed() < Duration::from_secs(5));
        drop(child.input);
        // Keep a validated Node child suspended: real pipe read deadline/drop test.
        let child = launch(&store.resources, &store.inbox, store.profile.clone()).unwrap();
        let super::super::pipe::ChildOwner::Windows(process) =
            super::super::pipe::ChildOwner::Windows(child.process)
        else {
            unreachable!()
        };
        let process = std::sync::Arc::new(std::sync::Mutex::new(
            super::super::pipe::ChildOwner::Windows(process),
        ));
        let mut pipe =
            PrivatePipe::from_io(process, Box::new(child.input), Box::new(child.output)).unwrap();
        let start = Instant::now();
        assert!(
            pipe.send(zeroize::Zeroizing::new(
                br#"{"v":1,"id":1,"op":"list"}"#.to_vec()
            ))
            .is_err()
        );
        drop(pipe);
        assert!(
            start.elapsed() >= Duration::from_secs(30) && start.elapsed() < Duration::from_secs(35)
        );
    }
    #[test]
    #[ignore = "Invoked only by synthetic parent to measure actual parent exit"]
    fn synthetic_parent_death_supervisor() {
        let observer_root = std::env::current_dir().unwrap();
        assert!(
            observer_root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("shixu synthetic "),
            "fixed parent fixture required"
        );
        let (store, base) = fixture();
        let child = launch(&store.resources, &store.inbox, store.profile.clone()).unwrap();
        Suspended::resume_thread(&child.thread).unwrap();
        let profile =
            String::from_utf16(&store.profile.name[..store.profile.name.len() - 1]).unwrap();
        assert!(store.profile.created);
        let metadata = serde_json::to_vec(&serde_json::json!({"pid":unsafe { GetProcessId(raw(&child.process.process)) },"profile":profile,"fixture":base})).unwrap();
        let observer_owner = current_sid().unwrap();
        create_file(
            &observer_root.join("parent-death.pending"),
            &descriptor(
                &observer_owner,
                &sid_text(store.profile.sid()).unwrap(),
                Access::Private,
                false,
            )
            .unwrap(),
            &metadata,
        )
        .unwrap();
        let from = path_wide(&observer_root.join("parent-death.pending")).unwrap();
        let to = path_wide(&observer_root.join("parent-death.json")).unwrap();
        assert_ne!(
            unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) },
            0
        );
        let mut ack = [0; 1];
        std::io::stdin().read_exact(&mut ack).unwrap();
        assert_eq!(ack[0], 1);
        // Deliberately skip Rust destructors: OS process exit closes the parent's
        // last Job handle. The external original test parent observes the child.
        std::process::exit(0);
    }
    fn actual_parent_exit(base: &Path) {
        use std::os::windows::process::CommandExt;
        let executable = std::env::current_exe().unwrap();
        let _program_guard = open_handle(&executable, false, false, FILE_SHARE_READ).unwrap();
        let mut supervisor = std::process::Command::new(executable)
            .env_clear()
            .current_dir(base)
            .args([
                "vault::windows::tests::synthetic_parent_death_supervisor",
                "--exact",
                "--ignored",
                "--test-threads=1",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        let metadata_path = base.join("parent-death.json");
        let deadline = Instant::now() + Duration::from_secs(30);
        while !metadata_path.exists()
            && Instant::now() < deadline
            && supervisor.try_wait().unwrap().is_none()
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        if !metadata_path.exists() {
            let _ = supervisor.kill();
            let _ = supervisor.wait();
            panic!("BLOCKED: actual parent-exit supervisor fixture failed");
        }
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(metadata_path).unwrap()).unwrap();
        let process = owned(unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                0,
                u32::try_from(metadata["pid"].as_u64().unwrap()).unwrap(),
            )
        })
        .unwrap();
        assert_eq!(
            unsafe { WaitForSingleObject(raw(&process), 0) },
            WAIT_TIMEOUT,
            "contained child alive before parent exit"
        );
        supervisor.stdin.take().unwrap().write_all(&[1]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let result = loop {
            if let Some(status) = supervisor.try_wait().unwrap() {
                break Some(status);
            }
            if Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        if result.is_none() {
            let _ = supervisor.kill();
            let _ = supervisor.wait();
        }
        assert!(
            result.is_some_and(|status| status.success()),
            "fixed supervisor did not exit successfully"
        );
        assert_eq!(
            unsafe { WaitForSingleObject(raw(&process), 4000) },
            WAIT_OBJECT_0,
            "actual parent exit must kill contained helper"
        );
        let profile_name = metadata["profile"].as_str().unwrap();
        assert!(profile_name.starts_with("shixu.synthetic."));
        let profile = wide(std::ffi::OsStr::new(profile_name)).unwrap();
        // This exact fixed supervisor reports only its newly created profile;
        // never enumerate/delete preexisting profiles.
        assert_eq!(unsafe { DeleteAppContainerProfile(profile.as_ptr()) }, 0);
        let fixture = PathBuf::from(metadata["fixture"].as_str().unwrap());
        assert!(
            fixture.parent() == Some(std::env::temp_dir().as_path())
                && fixture
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("shixu synthetic ")
        );
        std::fs::remove_dir_all(fixture).unwrap();
    }
    #[test]
    #[ignore = "Requires prepared actual Windows x64 synthetic-only execution"]
    fn synthetic_native_boundary() {
        production_constructor_remains_blocked();
        let mut blocked = vec![
            "second_identity_owner",
            "physical_power_loss",
            "physical_storage_full",
        ];
        let mut reparse_passed = false;
        let (store, base) = fixture();
        let mut engine = KdbxWebEngine::synthetic_windows(store);
        let master = "synthetic-only-master-v1";
        for newline in ["\r", "\n", "\u{85}", "\u{2028}", "\u{2029}"] {
            assert_eq!(
                engine.create(secret(&format!("x{newline}y"))),
                Err(AppError::InvalidInput)
            );
        }
        let cancelled = engine.cancellation().unwrap();
        cancelled.cancel();
        assert_eq!(engine.create(secret(master)), Err(AppError::Locked));
        engine.arm(VaultCancellation::default());
        engine
            .create(secret(master))
            .expect("BLOCKED: actual AppContainer Node/KdbxWeb create");
        let created = engine
            .apply(VaultMutation::Create {
                channel: "synthetic\r\n空 格".into(),
                account: "synthetic\r\naccount".into(),
                password: secret("synthetic-only-password"),
            })
            .unwrap();
        assert!(
            engine.list().unwrap() == vec![created.clone()],
            "fixed summary roundtrip"
        );
        assert!(
            engine
                .read_secret(&created.entry_id.to_string())
                .unwrap()
                .expose()
                == b"synthetic-only-password",
            "fixed secret roundtrip"
        );
        engine.close().unwrap();
        engine.open(secret(master)).unwrap();
        assert_eq!(
            engine.apply(VaultMutation::Delete {
                id: created.entry_id,
                expected_revision: 0
            }),
            Err(AppError::Conflict)
        );
        engine.open(secret(master)).unwrap();
        let updated = engine
            .apply(VaultMutation::Update {
                id: created.entry_id,
                expected_revision: 1,
                channel: created.channel,
                account: created.account,
                password: secret("synthetic-only-password-v2"),
            })
            .unwrap();
        assert_eq!(updated.revision, 2);
        for newline in ["\r", "\n", "\u{85}", "\u{2028}", "\u{2029}"] {
            assert_eq!(
                engine.change_master(secret(master), secret(&format!("x{newline}y"))),
                Err(AppError::InvalidInput)
            );
            engine.open(secret(master)).unwrap();
            assert!(
                engine
                    .apply(VaultMutation::Create {
                        channel: "c".into(),
                        account: "a\r\n".into(),
                        password: secret(&format!("x{newline}y"))
                    })
                    .is_err()
            );
            engine.open(secret(master)).unwrap();
        }
        engine
            .change_master(secret(master), secret("synthetic-only-master-v2"))
            .unwrap();
        engine.close().unwrap();
        assert_eq!(engine.open(secret(master)), Err(AppError::AuthFailed));
        engine.open(secret("synthetic-only-master-v2")).unwrap();
        engine.close().unwrap();
        diagnostic(engine.windows.as_mut().unwrap(), &base);
        actual_parent_exit(&base);
        resource_failures(&base, engine.windows.as_ref().unwrap());
        job_close_and_blocked_io(engine.windows.as_ref().unwrap());
        // Actual replacement failure injection runs the same storage implementation.
        for failure in [
            Failure::BeforeCheckpoint,
            Failure::AfterCheckpoint,
            Failure::BeforePending,
            Failure::AfterPending,
            Failure::BeforeReplace,
            Failure::AfterReplace,
            Failure::BeforeFinalFlush,
            Failure::AfterFinalFlush,
        ] {
            engine.open(secret("synthetic-only-master-v2")).unwrap();
            let before = std::fs::read(base.join("active").join("vault.kdbx")).unwrap();
            engine.windows.as_mut().unwrap().failure = failure;
            assert!(
                engine
                    .apply(VaultMutation::Create {
                        channel: "failure".into(),
                        account: "synthetic".into(),
                        password: secret("synthetic-only")
                    })
                    .is_err()
            );
            assert_eq!(engine.list(), Err(AppError::Locked));
            engine.windows.as_mut().unwrap().failure = Failure::None;
            let store = engine.windows.as_ref().unwrap();
            if failure != Failure::BeforeCheckpoint {
                let retained = std::fs::read_dir(&store.root)
                    .unwrap()
                    .filter_map(Result::ok)
                    .any(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with("vault.checkpoint-")
                            && std::fs::read(entry.path()).ok().as_deref() == Some(&before)
                    });
                assert!(retained, "old encrypted checkpoint retained");
                checkpoint_decrypt(store, &before);
            }
            engine.open(secret("synthetic-only-master-v2")).unwrap();
            engine.close().unwrap();
        }
        engine.open(secret("synthetic-only-master-v2")).unwrap();
        engine
            .apply(VaultMutation::Delete {
                id: updated.entry_id,
                expected_revision: updated.revision,
            })
            .unwrap();
        engine.close().unwrap();
        let store = engine.windows.as_ref().unwrap();
        let mut hardlink = store
            .admit(&store.root.join("vault.kdbx"), Access::Private, false)
            .unwrap();
        let data = read(&mut hardlink, MAX_FILE).unwrap();
        drop(hardlink);
        assert!(
            !data
                .windows(b"synthetic-only".len())
                .any(|chunk| chunk == b"synthetic-only")
        );
        std::fs::hard_link(
            store.root.join("vault.kdbx"),
            store.root.join("hardlink.kdbx"),
        )
        .unwrap();
        assert!(
            store
                .admit(&store.root.join("vault.kdbx"), Access::Private, false)
                .is_err()
        );
        std::fs::remove_file(store.root.join("hardlink.kdbx")).unwrap();
        let link = base.join("reparse-link");
        match std::os::windows::fs::symlink_dir(base.join("active"), &link) {
            Ok(()) => {
                assert!(
                    open_handle(&link, true, false, FILE_SHARE_READ | FILE_SHARE_WRITE).is_err()
                );
                assert!(ancestors(&link.join("nested")).is_err());
                std::fs::remove_dir(&link).unwrap();
                let leaf = base.join("reparse-leaf.kdbx");
                std::os::windows::fs::symlink_file(base.join("active").join("vault.kdbx"), &leaf)
                    .unwrap();
                assert!(open_handle(&leaf, false, false, FILE_SHARE_READ).is_err());
                std::fs::remove_file(leaf).unwrap();
                reparse_passed = true;
            }
            Err(error) if error.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD as i32) => {
                blocked.push("reparse_fixture_privilege")
            }
            Err(_) => panic!("reparse fixture creation failed"),
        }
        engine.open(secret("synthetic-only-master-v2")).unwrap();
        let active = base.join("active").join("vault.kdbx");
        let saved = std::fs::read(&active).unwrap();
        std::fs::write(&active, b"fixed external edit").unwrap();
        assert_eq!(engine.list(), Err(AppError::Conflict));
        assert_eq!(engine.list(), Err(AppError::Locked));
        std::fs::write(&active, saved).unwrap();
        // Cancellation during real Argon2 work closes process and bounds IO join.
        engine.open(secret("synthetic-only-master-v2")).unwrap();
        let cancel = engine.cancellation().unwrap();
        let killer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            cancel.cancel();
        });
        let start = Instant::now();
        assert!(
            engine
                .change_master(
                    secret("synthetic-only-master-v2"),
                    secret("synthetic-only-master-v3")
                )
                .is_err()
        );
        killer.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(35));
        engine.close().unwrap();
        let store = engine.windows.as_ref().unwrap();
        let cancel = VaultCancellation::default();
        let pipe = PrivatePipe::spawn_windows(store, cancel.clone()).unwrap();
        cancel.cancel();
        drop(pipe);
        drop(engine);
        std::fs::remove_dir_all(&base).unwrap();
        println!(
            "SHIXU_NATIVE_BOUNDARY {}",
            serde_json::json!({"passed_cases":["production_gate","actual_create","actual_crud","reopen","revision_conflict","wrong_master","change_master","newline_guards","token_job_before_resume","private_parent_os_denial","calendar_os_denial","resource_os_write_denial","inbox_os_write","exact_handle_semantic_denial","tcp_os_denial_with_control","udp_os_denial_with_control","one_process_actual_denial","memory_actual_denial","resource_exact_set_hash","resource_pinned_write_denial","broad_acl_denial","remote_path_denial","cooperating_lease","cooperating_external_edit","hardlink_denial","checkpoint_failures_decrypt","cancel_before_start", "cancel_late_registration_before_resume","cancel_during_kdf","job_close_termination", "actual_parent_exit_termination","blocked_io_cleanup","timeout_30s_drop_bounded"],"blocked_cases":blocked,"reparse_fixture_passed":reparse_passed,"production":"Unsupported"})
        );
    }
}
