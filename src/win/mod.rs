//! Thin safe wrappers over the Win32 and NT APIs the scanner needs. All `unsafe` FFI lives under this module.

pub mod handles;
pub mod memory;
pub mod process;
pub mod volume;

use std::ptr::null_mut;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;

/// A kernel handle that is closed on drop.
pub struct OwnedHandle(HANDLE);

impl OwnedHandle {
    /// Takes ownership of `raw`, or returns `None` if it is null or `INVALID_HANDLE_VALUE` (the two failure values Win32 uses).
    pub fn new(raw: HANDLE) -> Option<Self> {
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            None
        } else {
            Some(Self(raw))
        }
    }

    pub fn raw(&self) -> HANDLE {
        self.0
    }

    /// Gives up ownership without closing the handle.
    pub fn leak(mut self) {
        self.0 = null_mut();
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `self.0` is a valid handle owned by this value and closed exactly once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// Encodes `s` as a NUL-terminated UTF-16 string.
pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Decodes UTF-16 up to the first NUL (or the end of `buf`).
pub fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Decodes a NUL-terminated UTF-16 string from a raw pointer.
///
/// # Safety
/// `ptr` must be null or point to a NUL-terminated UTF-16 string.
pub unsafe fn from_wide_ptr(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0;
    // SAFETY: the caller guarantees the string is NUL-terminated.
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    }
}

/// Splits a `REG_MULTI_SZ`-style buffer (strings separated by NUL, ended by an empty string) into its parts.
pub fn split_multi_sz(buf: &[u16]) -> Vec<String> {
    buf.split(|&c| c == 0)
        .take_while(|s| !s.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_round_trip() {
        let wide = to_wide(r"D:\Epic Games");
        assert_eq!(wide.last(), Some(&0));
        assert_eq!(from_wide(&wide), r"D:\Epic Games");
    }

    #[test]
    fn multi_sz_stops_at_empty_string() {
        let buf: Vec<u16> = "D:\\\0E:\\mnt\\\0\0garbage\0".encode_utf16().collect();
        assert_eq!(split_multi_sz(&buf), vec![r"D:\", r"E:\mnt\"]);
    }

    #[test]
    fn owned_handle_rejects_failure_values() {
        assert!(OwnedHandle::new(null_mut()).is_none());
        assert!(OwnedHandle::new(INVALID_HANDLE_VALUE).is_none());
    }
}
