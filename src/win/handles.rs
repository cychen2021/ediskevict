//! Enumerates the file handles open in every process and resolves their paths.
//!
//! Works off the system-wide handle table (`NtQuerySystemInformation` with `SystemExtendedHandleInformation`). Each candidate handle is duplicated into this process and its name is queried on a worker thread, because querying some handles (synchronous named pipes) blocks forever; a worker that does not answer in time is abandoned together with its handle.

use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::mem::size_of;
use std::ptr::null;
use std::ptr::null_mut;
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::thread;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use windows_sys::Wdk::Foundation::NtQueryObject;
use windows_sys::Wdk::Foundation::OBJECT_INFORMATION_CLASS;
use windows_sys::Wdk::Foundation::OBJECT_NAME_INFORMATION;
use windows_sys::Wdk::System::SystemInformation::NtQuerySystemInformation;
use windows_sys::Wdk::System::SystemInformation::SYSTEM_INFORMATION_CLASS;
use windows_sys::Win32::Foundation::DUPLICATE_SAME_ACCESS;
use windows_sys::Win32::Foundation::DuplicateHandle;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::STATUS_INFO_LENGTH_MISMATCH;
use windows_sys::Win32::Storage::FileSystem::CreateFileW;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE;
use windows_sys::Win32::Storage::FileSystem::FILE_TYPE_DISK;
use windows_sys::Win32::Storage::FileSystem::GetFileType;
use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
use windows_sys::Win32::Storage::FileSystem::OPEN_EXISTING;
use windows_sys::Win32::Storage::FileSystem::VOLUME_NAME_NT;
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::System::Threading::OpenProcess;
use windows_sys::Win32::System::Threading::PROCESS_DUP_HANDLE;

use super::OwnedHandle;
use super::to_wide;

const SYSTEM_EXTENDED_HANDLE_INFORMATION: SYSTEM_INFORMATION_CLASS = 64;
const OBJECT_NAME_INFORMATION_CLASS: OBJECT_INFORMATION_CLASS = 1;
const NAME_QUERY_TIMEOUT: Duration = Duration::from_millis(500);

/// `SYSTEM_HANDLE_TABLE_ENTRY_INFO_EX`, which `windows-sys` does not define.
#[repr(C)]
#[derive(Clone, Copy)]
struct HandleEntry {
    object: usize,
    pid: usize,
    handle: usize,
    granted_access: u32,
    creator_back_trace_index: u16,
    object_type_index: u16,
    attributes: u32,
    reserved: u32,
}

/// The result of [`open_files`].
#[derive(Debug, Default)]
pub struct HandleScan {
    /// `(pid, NT path)` of every kept file handle.
    pub files: Vec<(u32, String)>,
    /// Processes whose handles could not be inspected (usually for lack of rights).
    pub inaccessible: HashSet<u32>,
    /// Handles whose name query hung and was abandoned.
    pub timed_out: usize,
}

/// Lists the open disk-file handles of all processes whose NT path (e.g. `\Device\HarddiskVolume7\Games\x.pak`) satisfies `keep`.
pub fn open_files(keep: impl Fn(&str) -> bool) -> Result<HandleScan> {
    // SAFETY: no preconditions.
    let self_pid = unsafe { GetCurrentProcessId() };
    // The numeric type index of "File" objects varies between Windows builds, so learn it from a handle we know is a file.
    let probe = open_probe_file()?;
    let entries = snapshot()?;
    let file_type = entries
        .iter()
        .find(|e| e.pid == self_pid as usize && e.handle == probe.raw() as usize)
        .map(|e| e.object_type_index)
        .context("cannot identify the File object type in the handle table")?;
    drop(probe);

    let mut processes: HashMap<u32, Option<OwnedHandle>> = HashMap::new();
    let mut querier = NameQuerier::new();
    let mut scan = HandleScan::default();
    for entry in entries.iter().filter(|e| e.object_type_index == file_type) {
        let pid = entry.pid as u32;
        // SAFETY: no preconditions; failure yields null, which `OwnedHandle::new` rejects.
        let process = processes.entry(pid).or_insert_with(|| {
            OwnedHandle::new(unsafe { OpenProcess(PROCESS_DUP_HANDLE, 0, pid) })
        });
        let Some(process) = process else {
            scan.inaccessible.insert(pid);
            continue;
        };
        let mut dup = null_mut();
        // SAFETY: `process` is a live handle with PROCESS_DUP_HANDLE; a stale source handle value just makes the call fail.
        let ok = unsafe {
            DuplicateHandle(
                process.raw(),
                entry.handle as HANDLE,
                GetCurrentProcess(),
                &mut dup,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        let Some(dup) = (ok != 0).then(|| OwnedHandle::new(dup)).flatten() else {
            continue;
        };
        match querier.name_of(&dup) {
            NameQuery::Name(name) => {
                if keep(&name) {
                    scan.files.push((pid, name));
                }
            }
            NameQuery::Skipped => {}
            NameQuery::TimedOut => {
                // The abandoned worker thread may still be using the handle, so it must stay open.
                dup.leak();
                scan.timed_out += 1;
            }
        }
    }
    Ok(scan)
}

fn open_probe_file() -> Result<OwnedHandle> {
    let exe = std::env::current_exe().context("cannot locate the running executable")?;
    let wide = to_wide(&exe.to_string_lossy());
    // SAFETY: `wide` is NUL-terminated; the other arguments are plain flags or null.
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    OwnedHandle::new(raw)
        .ok_or_else(io::Error::last_os_error)
        .context("cannot open the running executable")
}

fn snapshot() -> Result<Vec<HandleEntry>> {
    let mut len: usize = 4 << 20;
    loop {
        let mut buf = vec![0usize; len / size_of::<usize>()];
        let byte_len = buf.len() * size_of::<usize>();
        let mut needed = 0u32;
        // SAFETY: `buf` is writable for `byte_len` bytes and usize-aligned, which the structure requires.
        let status = unsafe {
            NtQuerySystemInformation(
                SYSTEM_EXTENDED_HANDLE_INFORMATION,
                buf.as_mut_ptr().cast(),
                byte_len as u32,
                &mut needed,
            )
        };
        if status == STATUS_INFO_LENGTH_MISMATCH {
            // The table grows between calls, so leave headroom.
            len = (needed as usize).max(byte_len) * 2;
            if len > u32::MAX as usize {
                bail!("the system handle table is too large");
            }
            continue;
        }
        if status < 0 {
            bail!(
                "NtQuerySystemInformation failed with NTSTATUS {:#010x}",
                status as u32
            );
        }
        // Layout: NumberOfHandles, Reserved, then the entries.
        let capacity = (buf.len() - 2) * size_of::<usize>() / size_of::<HandleEntry>();
        let count = buf[0].min(capacity);
        // SAFETY: the kernel wrote `count` entries right after the two header words, all within `buf`.
        let entries =
            unsafe { std::slice::from_raw_parts(buf.as_ptr().add(2).cast::<HandleEntry>(), count) };
        return Ok(entries.to_vec());
    }
}

enum NameQuery {
    /// The handle is a disk file or volume with this NT path.
    Name(String),
    /// The handle is not a disk file (e.g. a pipe or console), or it has no name.
    Skipped,
    /// The query hung; its worker was abandoned.
    TimedOut,
}

struct Worker {
    requests: mpsc::Sender<usize>,
    replies: mpsc::Receiver<Option<String>>,
}

impl Worker {
    fn spawn() -> Self {
        let (requests, request_rx) = mpsc::channel::<usize>();
        let (reply_tx, replies) = mpsc::channel();
        thread::spawn(move || {
            for handle in request_rx {
                // SAFETY: the requester keeps the handle open until the reply arrives, or leaks it on timeout.
                let name = unsafe { disk_file_name(handle as HANDLE) };
                if reply_tx.send(name).is_err() {
                    break;
                }
            }
        });
        Self { requests, replies }
    }
}

struct NameQuerier {
    worker: Worker,
}

impl NameQuerier {
    fn new() -> Self {
        Self {
            worker: Worker::spawn(),
        }
    }

    fn name_of(&mut self, handle: &OwnedHandle) -> NameQuery {
        if self.worker.requests.send(handle.raw() as usize).is_err() {
            self.worker = Worker::spawn();
            return NameQuery::Skipped;
        }
        match self.worker.replies.recv_timeout(NAME_QUERY_TIMEOUT) {
            Ok(Some(name)) => NameQuery::Name(name),
            Ok(None) => NameQuery::Skipped,
            Err(RecvTimeoutError::Timeout) => {
                self.worker = Worker::spawn();
                NameQuery::TimedOut
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.worker = Worker::spawn();
                NameQuery::Skipped
            }
        }
    }
}

/// Returns the NT path of `handle` if it refers to a disk file, directory or volume.
///
/// # Safety
/// `handle` must be a valid handle owned by this process.
unsafe fn disk_file_name(handle: HANDLE) -> Option<String> {
    // SAFETY: guaranteed by the caller.
    unsafe {
        if GetFileType(handle) != FILE_TYPE_DISK {
            return None;
        }
        final_path(handle).or_else(|| object_name(handle))
    }
}

/// `GetFinalPathNameByHandleW` in NT form. Fails for raw volume handles, which `object_name` covers.
unsafe fn final_path(handle: HANDLE) -> Option<String> {
    let mut buf = vec![0u16; 512];
    loop {
        // SAFETY: `buf` is writable for its full length; `handle` is valid per the caller.
        let len = unsafe {
            GetFinalPathNameByHandleW(handle, buf.as_mut_ptr(), buf.len() as u32, VOLUME_NAME_NT)
        } as usize;
        if len == 0 {
            return None;
        }
        if len >= buf.len() {
            buf.resize(len + 1, 0);
            continue;
        }
        return Some(String::from_utf16_lossy(&buf[..len]));
    }
}

unsafe fn object_name(handle: HANDLE) -> Option<String> {
    let mut buf = vec![0u64; 1024];
    let byte_len = (buf.len() * size_of::<u64>()) as u32;
    let mut returned = 0u32;
    // SAFETY: `buf` is writable for `byte_len` bytes and suitably aligned for OBJECT_NAME_INFORMATION.
    let status = unsafe {
        NtQueryObject(
            handle,
            OBJECT_NAME_INFORMATION_CLASS,
            buf.as_mut_ptr().cast(),
            byte_len,
            &mut returned,
        )
    };
    if status < 0 {
        return None;
    }
    // SAFETY: on success the buffer starts with an OBJECT_NAME_INFORMATION whose string points into the buffer.
    unsafe {
        let info = &*buf.as_ptr().cast::<OBJECT_NAME_INFORMATION>();
        if info.Name.Buffer.is_null() || info.Name.Length == 0 {
            return None;
        }
        let name = std::slice::from_raw_parts(info.Name.Buffer, info.Name.Length as usize / 2);
        Some(String::from_utf16_lossy(name))
    }
}
