//! Lists the files mapped into a process's memory: its executable, DLLs, and memory-mapped data files.
//!
//! These keep a volume busy even though the process holds no handle to them, because the loader closes the file handle after creating the mapping.

use std::collections::HashSet;
use std::mem::size_of;

use windows_sys::Win32::System::Memory::MEM_COMMIT;
use windows_sys::Win32::System::Memory::MEM_IMAGE;
use windows_sys::Win32::System::Memory::MEM_MAPPED;
use windows_sys::Win32::System::Memory::MEMORY_BASIC_INFORMATION;
use windows_sys::Win32::System::Memory::VirtualQueryEx;
use windows_sys::Win32::System::ProcessStatus::GetMappedFileNameW;
use windows_sys::Win32::System::Threading::OpenProcess;
use windows_sys::Win32::System::Threading::PROCESS_QUERY_INFORMATION;
use windows_sys::Win32::System::Threading::PROCESS_VM_READ;

use super::OwnedHandle;
use super::from_wide;

/// Returns the NT paths of all files mapped into process `pid`, or `None` if the process cannot be opened.
pub fn mapped_files(pid: u32) -> Option<Vec<String>> {
    // SAFETY: no preconditions; failure yields null.
    let process = OwnedHandle::new(unsafe {
        OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid)
    })?;
    let mut seen_bases = HashSet::new();
    let mut files = Vec::new();
    let mut address: usize = 0;
    let mut name = vec![0u16; 1024];
    loop {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: `info` is a writable MEMORY_BASIC_INFORMATION; any address is valid to query.
        let written = unsafe {
            VirtualQueryEx(
                process.raw(),
                address as *const _,
                &mut info,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if written == 0 {
            break;
        }
        let is_file = info.Type == MEM_IMAGE || info.Type == MEM_MAPPED;
        // One mapping spans several regions that share an allocation base; name it once.
        if info.State == MEM_COMMIT && is_file && seen_bases.insert(info.AllocationBase as usize) {
            // SAFETY: `name` is writable for its full length.
            let len = unsafe {
                GetMappedFileNameW(
                    process.raw(),
                    info.BaseAddress,
                    name.as_mut_ptr(),
                    name.len() as u32,
                )
            };
            if len != 0 {
                files.push(from_wide(&name[..len as usize]));
            }
        }
        match (info.BaseAddress as usize).checked_add(info.RegionSize) {
            Some(next) if next > address => address = next,
            _ => break,
        }
    }
    Some(files)
}
