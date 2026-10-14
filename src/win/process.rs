//! Process listing, metadata, privileges and termination.

use std::collections::HashMap;
use std::io;
use std::mem::size_of;
use std::ptr::null;
use std::ptr::null_mut;

use anyhow::Context;
use anyhow::Result;
use windows_sys::Win32::Foundation::ERROR_MORE_DATA;
use windows_sys::Win32::Foundation::LUID;
use windows_sys::Win32::Security::AdjustTokenPrivileges;
use windows_sys::Win32::Security::GetTokenInformation;
use windows_sys::Win32::Security::LUID_AND_ATTRIBUTES;
use windows_sys::Win32::Security::LookupPrivilegeValueW;
use windows_sys::Win32::Security::SE_DEBUG_NAME;
use windows_sys::Win32::Security::SE_PRIVILEGE_ENABLED;
use windows_sys::Win32::Security::TOKEN_ADJUST_PRIVILEGES;
use windows_sys::Win32::Security::TOKEN_ELEVATION;
use windows_sys::Win32::Security::TOKEN_PRIVILEGES;
use windows_sys::Win32::Security::TOKEN_QUERY;
use windows_sys::Win32::Security::TokenElevation;
use windows_sys::Win32::System::Diagnostics::ToolHelp::CreateToolhelp32Snapshot;
use windows_sys::Win32::System::Diagnostics::ToolHelp::PROCESSENTRY32W;
use windows_sys::Win32::System::Diagnostics::ToolHelp::Process32FirstW;
use windows_sys::Win32::System::Diagnostics::ToolHelp::Process32NextW;
use windows_sys::Win32::System::Diagnostics::ToolHelp::TH32CS_SNAPPROCESS;
use windows_sys::Win32::System::Services::CloseServiceHandle;
use windows_sys::Win32::System::Services::ENUM_SERVICE_STATUS_PROCESSW;
use windows_sys::Win32::System::Services::EnumServicesStatusExW;
use windows_sys::Win32::System::Services::OpenSCManagerW;
use windows_sys::Win32::System::Services::SC_ENUM_PROCESS_INFO;
use windows_sys::Win32::System::Services::SC_MANAGER_CONNECT;
use windows_sys::Win32::System::Services::SC_MANAGER_ENUMERATE_SERVICE;
use windows_sys::Win32::System::Services::SERVICE_ACTIVE;
use windows_sys::Win32::System::Services::SERVICE_WIN32;
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::IsProcessCritical;
use windows_sys::Win32::System::Threading::OpenProcess;
use windows_sys::Win32::System::Threading::OpenProcessToken;
use windows_sys::Win32::System::Threading::PROCESS_NAME_WIN32;
use windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION;
use windows_sys::Win32::System::Threading::PROCESS_TERMINATE;
use windows_sys::Win32::System::Threading::QueryFullProcessImageNameW;
use windows_sys::Win32::System::Threading::TerminateProcess;

use super::OwnedHandle;
use super::from_wide;
use super::from_wide_ptr;

/// A running process as listed by a Toolhelp snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessEntry {
    pub pid: u32,
    pub parent_pid: u32,
    pub name: String,
}

/// Lists all running processes.
pub fn list() -> Result<Vec<ProcessEntry>> {
    // SAFETY: no preconditions; failure yields INVALID_HANDLE_VALUE.
    let snapshot = OwnedHandle::new(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) })
        .ok_or_else(io::Error::last_os_error)
        .context("cannot list processes")?;
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut processes = Vec::new();
    // SAFETY: `entry` is a PROCESSENTRY32W with `dwSize` set, as the API requires.
    let mut ok = unsafe { Process32FirstW(snapshot.raw(), &mut entry) };
    while ok != 0 {
        processes.push(ProcessEntry {
            pid: entry.th32ProcessID,
            parent_pid: entry.th32ParentProcessID,
            name: from_wide(&entry.szExeFile),
        });
        // SAFETY: as above.
        ok = unsafe { Process32NextW(snapshot.raw(), &mut entry) };
    }
    Ok(processes)
}

fn open(pid: u32, access: u32) -> Option<OwnedHandle> {
    // SAFETY: no preconditions; failure yields null.
    OwnedHandle::new(unsafe { OpenProcess(access, 0, pid) })
}

/// Returns the full path of the executable of process `pid`, if it can be queried.
pub fn image_path(pid: u32) -> Option<String> {
    let process = open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` is writable for `len` characters.
    let ok = unsafe {
        QueryFullProcessImageNameW(
            process.raw(),
            PROCESS_NAME_WIN32,
            buf.as_mut_ptr(),
            &mut len,
        )
    };
    (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
}

/// Returns whether process `pid` is marked critical, i.e. ending it crashes Windows. `false` if it cannot be queried.
pub fn is_critical(pid: u32) -> bool {
    let Some(process) = open(pid, PROCESS_QUERY_LIMITED_INFORMATION) else {
        return false;
    };
    let mut critical = 0;
    // SAFETY: `critical` is a writable BOOL.
    let ok = unsafe { IsProcessCritical(process.raw(), &mut critical) };
    ok != 0 && critical != 0
}

fn current_token(access: u32) -> Option<OwnedHandle> {
    let mut token = null_mut();
    // SAFETY: `token` is writable; the pseudo-handle from GetCurrentProcess needs no closing.
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut token) };
    (ok != 0).then(|| OwnedHandle::new(token)).flatten()
}

/// Returns whether this process runs elevated (as Administrator).
pub fn is_elevated() -> bool {
    let Some(token) = current_token(TOKEN_QUERY) else {
        return false;
    };
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0;
    // SAFETY: `elevation` is writable for the size passed.
    let ok = unsafe {
        GetTokenInformation(
            token.raw(),
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    ok != 0 && elevation.TokenIsElevated != 0
}

/// Enables `SeDebugPrivilege`, which lets an elevated process open (and duplicate handles from) other users' processes and services. Does nothing if it cannot be enabled.
pub fn enable_debug_privilege() {
    let Some(token) = current_token(TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY) else {
        return;
    };
    let mut luid = LUID::default();
    // SAFETY: `luid` is writable; SE_DEBUG_NAME is a NUL-terminated constant.
    if unsafe { LookupPrivilegeValueW(null(), SE_DEBUG_NAME, &mut luid) } == 0 {
        return;
    }
    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    // SAFETY: `privileges` is a valid TOKEN_PRIVILEGES; the previous state is not requested.
    unsafe { AdjustTokenPrivileges(token.raw(), 0, &privileges, 0, null_mut(), null_mut()) };
}

/// Maps process IDs to the names of the running Windows services they host. Empty if the service manager cannot be queried.
pub fn services_by_pid() -> HashMap<u32, Vec<String>> {
    let mut services: HashMap<u32, Vec<String>> = HashMap::new();
    // SAFETY: null machine and database names select the local active database.
    let manager = unsafe {
        OpenSCManagerW(
            null(),
            null(),
            SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE,
        )
    };
    if manager.is_null() {
        return services;
    }
    let mut buf: Vec<u64> = vec![0; 8 * 1024];
    let mut resume = 0u32;
    loop {
        let mut needed = 0u32;
        let mut returned = 0u32;
        let byte_len = (buf.len() * size_of::<u64>()) as u32;
        // SAFETY: `buf` is writable for `byte_len` bytes and aligned for ENUM_SERVICE_STATUS_PROCESSW.
        let ok = unsafe {
            EnumServicesStatusExW(
                manager,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_ACTIVE,
                buf.as_mut_ptr().cast(),
                byte_len,
                &mut needed,
                &mut returned,
                &mut resume,
                null(),
            )
        };
        let more =
            ok == 0 && io::Error::last_os_error().raw_os_error() == Some(ERROR_MORE_DATA as i32);
        // SAFETY: the API wrote `returned` entries at the start of `buf`; their strings point into `buf`.
        let entries = unsafe {
            std::slice::from_raw_parts(
                buf.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>(),
                returned as usize,
            )
        };
        for entry in entries {
            // SAFETY: as above.
            let name = unsafe { from_wide_ptr(entry.lpServiceName) };
            services
                .entry(entry.ServiceStatusProcess.dwProcessId)
                .or_default()
                .push(name);
        }
        if !more {
            break;
        }
        if returned == 0 {
            // Nothing fit; grow the buffer before resuming.
            buf.resize((needed as usize).div_ceil(size_of::<u64>()) + buf.len(), 0);
        }
    }
    // SAFETY: `manager` is a valid service manager handle, closed once.
    unsafe { CloseServiceHandle(manager) };
    services
}

/// Ends process `pid`.
pub fn terminate(pid: u32) -> Result<()> {
    let process = open(pid, PROCESS_TERMINATE)
        .ok_or_else(io::Error::last_os_error)
        .with_context(|| format!("cannot open process {pid}"))?;
    // SAFETY: `process` is a live handle with PROCESS_TERMINATE.
    if unsafe { TerminateProcess(process.raw(), 1) } == 0 {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("cannot end process {pid}"));
    }
    Ok(())
}
