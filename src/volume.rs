//! Identifies the target volume and matches NT paths against it.
//!
//! A volume can be named by a drive letter, a folder mount, its `\\?\Volume{GUID}\` path, or (as the kernel reports open files and mappings) its NT device name such as `\Device\HarddiskVolume7`. Matching is done on the device name.

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;

use crate::win::volume as winvol;

/// A mounted volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    /// `\\?\Volume{GUID}\`.
    pub guid_path: String,
    /// NT device name, e.g. `\Device\HarddiskVolume7`.
    pub device: String,
    /// Mount points such as `D:\`; may be empty.
    pub mount_points: Vec<String>,
}

impl Volume {
    /// Looks up the volume named by `spec`: a drive letter (`D`, `D:`, `D:\`), any absolute path on the volume, or a `\\?\Volume{GUID}\` path.
    pub fn resolve(spec: &str) -> Result<Volume> {
        let spec = normalize_spec(spec)?;
        let mount_point = if is_guid_path(&spec) {
            spec.clone()
        } else {
            winvol::mount_point_of(&spec)?
        };
        let guid_path = winvol::guid_path_of(&mount_point)
            .with_context(|| format!("cannot resolve volume {spec}"))?;
        let device = winvol::device_of(&guid_path)?;
        let mount_points = winvol::mount_points_of(&guid_path);
        Ok(Volume {
            guid_path,
            device,
            mount_points,
        })
    }

    /// Returns whether Windows itself is installed on this volume.
    pub fn is_system(&self) -> Result<bool> {
        let windows_mount = winvol::mount_point_of(&winvol::windows_dir()?)?;
        let windows_guid = winvol::guid_path_of(&windows_mount)?;
        Ok(windows_guid.eq_ignore_ascii_case(&self.guid_path))
    }

    /// A short name for messages: `D:` for a drive letter, otherwise the first mount point or the GUID path.
    pub fn label(&self) -> String {
        match self.mount_points.first() {
            Some(mount) if is_drive_root(mount) => mount.trim_end_matches('\\').to_string(),
            Some(mount) => mount.clone(),
            None => self.guid_path.clone(),
        }
    }

    /// Returns whether the NT path `nt_path` is on this volume.
    pub fn contains(&self, nt_path: &str) -> bool {
        strip_device(nt_path, &self.device).is_some()
    }

    /// Rewrites an NT path on this volume into a user-facing one (`\Device\HarddiskVolume7\x` → `D:\x`). Other paths are returned unchanged.
    pub fn display_path(&self, nt_path: &str) -> String {
        let Some(rest) = strip_device(nt_path, &self.device) else {
            return nt_path.to_string();
        };
        let base = self
            .mount_points
            .first()
            .unwrap_or(&self.guid_path)
            .trim_end_matches('\\');
        let rest = if rest.is_empty() { "\\" } else { rest };
        format!("{base}{rest}")
    }
}

/// Canonicalizes a user-supplied volume spec: drive letters become `X:\`, slashes become backslashes, and GUID paths get a trailing backslash. Relative paths are rejected.
pub fn normalize_spec(spec: &str) -> Result<String> {
    let spec = spec.trim().replace('/', "\\");
    if is_guid_path(&spec) {
        return Ok(if spec.ends_with('\\') {
            spec
        } else {
            format!("{spec}\\")
        });
    }
    let mut chars = spec.chars();
    if let Some(letter) = chars.next().filter(char::is_ascii_alphabetic) {
        let letter = letter.to_ascii_uppercase();
        let rest = chars.as_str();
        if rest.is_empty() || rest == ":" {
            return Ok(format!("{letter}:\\"));
        }
        if let Some(path) = rest.strip_prefix(":\\") {
            return Ok(format!("{letter}:\\{path}"));
        }
    }
    bail!(
        r"unrecognized volume {spec:?}: expected a drive letter (D:), an absolute path on the volume (D:\Games), or a volume GUID path (\\?\Volume{{...}}\)"
    )
}

fn is_guid_path(spec: &str) -> bool {
    spec.get(..10)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(r"\\?\Volume"))
}

fn is_drive_root(mount: &str) -> bool {
    let bytes = mount.as_bytes();
    bytes.len() == 3 && bytes[0].is_ascii_alphabetic() && &bytes[1..] == b":\\"
}

/// If `path` is `device` or inside it, returns the remainder (empty or starting with `\`). Case-insensitive.
fn strip_device<'a>(path: &'a str, device: &str) -> Option<&'a str> {
    let head = path.get(..device.len())?;
    let rest = &path[device.len()..];
    (head.eq_ignore_ascii_case(device) && (rest.is_empty() || rest.starts_with('\\')))
        .then_some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(mount_points: &[&str]) -> Volume {
        Volume {
            guid_path: r"\\?\Volume{0000-1111}\".to_string(),
            device: r"\Device\HarddiskVolume7".to_string(),
            mount_points: mount_points.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn normalizes_drive_letters() {
        for spec in ["d", "D", "d:", "D:", r"D:\", "D:/", " D: "] {
            assert_eq!(normalize_spec(spec).unwrap(), r"D:\", "spec {spec:?}");
        }
    }

    #[test]
    fn normalizes_paths_and_guid_paths() {
        assert_eq!(
            normalize_spec("d:/Epic Games/Fortnite").unwrap(),
            r"D:\Epic Games\Fortnite"
        );
        assert_eq!(
            normalize_spec(r"\\?\Volume{abc}").unwrap(),
            r"\\?\Volume{abc}\"
        );
        assert_eq!(
            normalize_spec(r"\\?\volume{abc}\").unwrap(),
            r"\\?\volume{abc}\"
        );
    }

    #[test]
    fn rejects_relative_and_unknown_specs() {
        for spec in [
            "",
            "not-a-volume",
            "D:Games",
            "DE:",
            r"\\server\share",
            "1:",
        ] {
            assert!(normalize_spec(spec).is_err(), "spec {spec:?}");
        }
    }

    #[test]
    fn contains_matches_whole_path_components_case_insensitively() {
        let v = volume(&[r"D:\"]);
        assert!(v.contains(r"\Device\HarddiskVolume7"));
        assert!(v.contains(r"\Device\HarddiskVolume7\"));
        assert!(v.contains(r"\device\harddiskvolume7\Games\x.pak"));
        assert!(!v.contains(r"\Device\HarddiskVolume71\Games\x.pak"));
        assert!(!v.contains(r"\Device\HarddiskVolume3\Windows"));
        assert!(!v.contains(r"\Device\Harddisk"));
    }

    #[test]
    fn display_path_uses_first_mount_point() {
        let v = volume(&[r"D:\", r"C:\mnt\ssd\"]);
        assert_eq!(
            v.display_path(r"\Device\HarddiskVolume7\Games\x.pak"),
            r"D:\Games\x.pak"
        );
        assert_eq!(v.display_path(r"\Device\HarddiskVolume7"), r"D:\");
        assert_eq!(
            v.display_path(r"\Device\HarddiskVolume3\Windows"),
            r"\Device\HarddiskVolume3\Windows"
        );
    }

    #[test]
    fn display_path_falls_back_to_guid_path() {
        let v = volume(&[]);
        assert_eq!(
            v.display_path(r"\Device\HarddiskVolume7\x"),
            r"\\?\Volume{0000-1111}\x"
        );
    }

    #[test]
    fn label_prefers_drive_letter() {
        assert_eq!(volume(&[r"D:\"]).label(), "D:");
        assert_eq!(volume(&[r"C:\mnt\ssd\"]).label(), r"C:\mnt\ssd\");
        assert_eq!(volume(&[]).label(), r"\\?\Volume{0000-1111}\");
    }
}
