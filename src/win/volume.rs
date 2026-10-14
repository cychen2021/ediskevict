//! Volume name lookups: mount point ↔ volume GUID path ↔ NT device name.

use std::io;

use anyhow::Context;
use anyhow::Result;
use windows_sys::Win32::Foundation::ERROR_MORE_DATA;
use windows_sys::Win32::Storage::FileSystem::GetVolumeNameForVolumeMountPointW;
use windows_sys::Win32::Storage::FileSystem::GetVolumePathNameW;
use windows_sys::Win32::Storage::FileSystem::GetVolumePathNamesForVolumeNameW;
use windows_sys::Win32::Storage::FileSystem::QueryDosDeviceW;
use windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;

use super::from_wide;
use super::split_multi_sz;
use super::to_wide;

/// Returns the mount point (e.g. `D:\`) of the volume that `path` is on.
pub fn mount_point_of(path: &str) -> Result<String> {
    let wide = to_wide(path);
    let mut buf = vec![0u16; 1024];
    // SAFETY: `wide` is NUL-terminated and `buf` is writable for its full length.
    let ok = unsafe { GetVolumePathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if ok == 0 {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("cannot find the volume of {path}"));
    }
    Ok(from_wide(&buf))
}

/// Returns the `\\?\Volume{GUID}\` path of the volume mounted at `mount_point` (which must end with `\`).
pub fn guid_path_of(mount_point: &str) -> Result<String> {
    let wide = to_wide(mount_point);
    let mut buf = vec![0u16; 64];
    // SAFETY: `wide` is NUL-terminated and `buf` is writable for its full length.
    let ok = unsafe {
        GetVolumeNameForVolumeMountPointW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32)
    };
    if ok == 0 {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("{mount_point} is not a mounted volume"));
    }
    Ok(from_wide(&buf))
}

/// Returns the NT device name (e.g. `\Device\HarddiskVolume7`) behind a `\\?\Volume{GUID}\` path.
pub fn device_of(guid_path: &str) -> Result<String> {
    let name = guid_path.trim_start_matches(r"\\?\").trim_end_matches('\\');
    let wide = to_wide(name);
    let mut buf = vec![0u16; 1024];
    // SAFETY: `wide` is NUL-terminated and `buf` is writable for its full length.
    let len = unsafe { QueryDosDeviceW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if len == 0 {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("cannot find the device of {guid_path}"));
    }
    split_multi_sz(&buf[..len as usize])
        .into_iter()
        .next()
        .with_context(|| format!("{guid_path} has no device name"))
}

/// Returns every mount point (drive letters and folder mounts) of a `\\?\Volume{GUID}\` path. Empty if it has none or the lookup fails.
pub fn mount_points_of(guid_path: &str) -> Vec<String> {
    let wide = to_wide(guid_path);
    let mut buf = vec![0u16; 256];
    loop {
        let mut needed = 0u32;
        // SAFETY: `wide` is NUL-terminated and `buf` is writable for its full length.
        let ok = unsafe {
            GetVolumePathNamesForVolumeNameW(
                wide.as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut needed,
            )
        };
        if ok != 0 {
            return split_multi_sz(&buf);
        }
        if io::Error::last_os_error().raw_os_error() != Some(ERROR_MORE_DATA as i32)
            || needed as usize <= buf.len()
        {
            return Vec::new();
        }
        buf.resize(needed as usize, 0);
    }
}

/// Returns the Windows directory, e.g. `C:\Windows`.
pub fn windows_dir() -> Result<String> {
    let mut buf = vec![0u16; 1024];
    // SAFETY: `buf` is writable for its full length.
    let len = unsafe { GetSystemWindowsDirectoryW(buf.as_mut_ptr(), buf.len() as u32) };
    if len == 0 || len as usize >= buf.len() {
        return Err(io::Error::last_os_error()).context("cannot find the Windows directory");
    }
    Ok(from_wide(&buf))
}
