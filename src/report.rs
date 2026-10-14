//! Groups raw findings into per-process holders, decides which may be killed, and renders the report.

use std::collections::BTreeMap;
use std::fmt::Write;

/// How a process holds a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldKind {
    /// An open file or directory handle (including the process's working directory).
    OpenHandle,
    /// A file mapped into memory: the executable, a DLL, or a memory-mapped data file.
    Mapped,
}

/// One file on the volume held by one process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub pid: u32,
    pub kind: HoldKind,
    /// User-facing path, e.g. `D:\Games\x.pak`.
    pub path: String,
}

/// What is known about a process besides its findings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessInfo {
    pub name: String,
    pub image: Option<String>,
    /// Windows services hosted by the process.
    pub services: Vec<String>,
    /// Ending the process would crash Windows.
    pub critical: bool,
    /// Creation time from `process::start_time`; `None` if it could not be queried.
    pub started: Option<u64>,
}

/// A process holding files on the volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    pub info: ProcessInfo,
    /// Sorted, deduplicated.
    pub open_files: Vec<String>,
    /// Sorted, deduplicated.
    pub mapped_files: Vec<String>,
}

/// The processes the tool must treat specially when deciding what may be killed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    /// This process.
    pub self_pid: u32,
    /// The process that started this one, usually the user's shell.
    pub parent_pid: Option<u32>,
}

impl Holder {
    /// Returns why this process must not be killed, or `None` if killing it is allowed.
    pub fn kill_refusal(&self, context: Context) -> Option<String> {
        if self.pid == context.self_pid {
            return Some("this is ediskevict itself; run it from another drive".to_string());
        }
        if Some(self.pid) == context.parent_pid {
            return Some(
                "this is the shell running ediskevict; `cd` to another drive in it instead"
                    .to_string(),
            );
        }
        if self.pid <= 4 {
            return Some("the System process belongs to Windows itself".to_string());
        }
        if self.info.critical {
            return Some(
                "it is a critical Windows process; ending it would crash Windows".to_string(),
            );
        }
        if !self.info.services.is_empty() {
            let commands: Vec<String> = self
                .info
                .services
                .iter()
                .map(|s| format!("`sc stop {s}`"))
                .collect();
            return Some(format!(
                "it hosts Windows services; stop them instead with {}",
                commands.join(", ")
            ));
        }
        if self.info.started.is_none() {
            return Some("its start time cannot be read, so it cannot be told apart from a later process reusing its PID".to_string());
        }
        None
    }
}

/// Groups findings by process. Holders are sorted by process name, then PID.
pub fn group(findings: Vec<Finding>, mut info: impl FnMut(u32) -> ProcessInfo) -> Vec<Holder> {
    let mut by_pid: BTreeMap<u32, (Vec<String>, Vec<String>)> = BTreeMap::new();
    for finding in findings {
        let (open, mapped) = by_pid.entry(finding.pid).or_default();
        match finding.kind {
            HoldKind::OpenHandle => open.push(finding.path),
            HoldKind::Mapped => mapped.push(finding.path),
        }
    }
    let mut holders: Vec<Holder> = by_pid
        .into_iter()
        .map(|(pid, (open_files, mapped_files))| Holder {
            pid,
            info: info(pid),
            open_files: sorted_unique(open_files),
            mapped_files: sorted_unique(mapped_files),
        })
        .collect();
    holders.sort_by(|a, b| {
        a.info
            .name
            .to_lowercase()
            .cmp(&b.info.name.to_lowercase())
            .then(a.pid.cmp(&b.pid))
    });
    holders
}

fn sorted_unique(mut paths: Vec<String>) -> Vec<String> {
    paths.sort_by_key(|p| p.to_lowercase());
    paths.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    paths
}

/// How many paths per category to print unless `verbose`.
pub const PATHS_SHOWN: usize = 5;

/// Renders the holders of `label` (e.g. `D:`) as a human-readable report.
pub fn render(holders: &[Holder], label: &str, context: Context, verbose: bool) -> String {
    let mut out = String::new();
    if holders.is_empty() {
        let _ = writeln!(out, "No process is holding files on {label}.");
        return out;
    }
    let noun = if holders.len() == 1 {
        "process is"
    } else {
        "processes are"
    };
    let _ = writeln!(out, "{} {noun} using {label}", holders.len());
    for holder in holders {
        let _ = writeln!(out);
        let _ = write!(out, "{} (PID {})", holder.info.name, holder.pid);
        if !holder.info.services.is_empty() {
            let _ = write!(out, " [services: {}]", holder.info.services.join(", "));
        }
        let _ = writeln!(out);
        if let Some(image) = &holder.info.image {
            let _ = writeln!(out, "  program: {image}");
        }
        render_paths(&mut out, "open:   ", &holder.open_files, verbose);
        render_paths(&mut out, "loaded: ", &holder.mapped_files, verbose);
        if let Some(reason) = holder.kill_refusal(context) {
            let _ = writeln!(out, "  cannot be killed: {reason}");
        }
    }
    out
}

fn render_paths(out: &mut String, title: &str, paths: &[String], verbose: bool) {
    let shown = if verbose {
        paths.len()
    } else {
        paths.len().min(PATHS_SHOWN)
    };
    for (i, path) in paths[..shown].iter().enumerate() {
        let title = if i == 0 { title } else { "        " };
        let _ = writeln!(out, "  {title} {path}");
    }
    if shown < paths.len() {
        let _ = writeln!(
            out,
            "           ... and {} more (use --verbose to list all)",
            paths.len() - shown
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTEXT: Context = Context {
        self_pid: 100,
        parent_pid: Some(50),
    };

    fn finding(pid: u32, kind: HoldKind, path: &str) -> Finding {
        Finding {
            pid,
            kind,
            path: path.to_string(),
        }
    }

    fn info(name: &str) -> ProcessInfo {
        ProcessInfo {
            name: name.to_string(),
            started: Some(1),
            ..Default::default()
        }
    }

    fn holder(pid: u32, info: ProcessInfo) -> Holder {
        Holder {
            pid,
            info,
            open_files: vec![r"D:\x".to_string()],
            mapped_files: Vec::new(),
        }
    }

    #[test]
    fn group_merges_sorts_and_dedups() {
        let findings = vec![
            finding(7, HoldKind::OpenHandle, r"D:\b"),
            finding(3, HoldKind::Mapped, r"D:\Game\game.exe"),
            finding(7, HoldKind::OpenHandle, r"D:\a"),
            finding(7, HoldKind::OpenHandle, r"d:\B"),
            finding(7, HoldKind::Mapped, r"D:\lib.dll"),
        ];
        let holders = group(findings, |pid| {
            info(if pid == 3 {
                "Zgame.exe"
            } else {
                "EpicGamesLauncher.exe"
            })
        });
        assert_eq!(holders.len(), 2);
        assert_eq!(holders[0].pid, 7);
        assert_eq!(holders[0].open_files, vec![r"D:\a", r"D:\b"]);
        assert_eq!(holders[0].mapped_files, vec![r"D:\lib.dll"]);
        assert_eq!(holders[1].pid, 3);
        assert_eq!(holders[1].mapped_files, vec![r"D:\Game\game.exe"]);
    }

    #[test]
    fn ordinary_process_may_be_killed() {
        assert_eq!(
            holder(1234, info("EpicGamesLauncher.exe")).kill_refusal(CONTEXT),
            None
        );
    }

    #[test]
    fn protected_processes_are_refused() {
        let critical = ProcessInfo {
            critical: true,
            ..info("csrss.exe")
        };
        let service = ProcessInfo {
            services: vec!["WSearch".to_string()],
            ..info("SearchIndexer.exe")
        };
        let cases = [
            (holder(100, info("ediskevict.exe")), "ediskevict itself"),
            (holder(50, info("pwsh.exe")), "shell"),
            (holder(4, info("System")), "System"),
            (holder(600, critical), "critical"),
            (holder(700, service), "`sc stop WSearch`"),
            (
                holder(
                    800,
                    ProcessInfo {
                        started: None,
                        ..info("game.exe")
                    },
                ),
                "start time",
            ),
        ];
        for (holder, expected) in cases {
            let reason = holder.kill_refusal(CONTEXT).expect("should be refused");
            assert!(
                reason.contains(expected),
                "{reason:?} should mention {expected:?}"
            );
        }
    }

    #[test]
    fn render_reports_empty_volume() {
        assert_eq!(
            render(&[], "D:", CONTEXT, false),
            "No process is holding files on D:.\n"
        );
    }

    #[test]
    fn render_lists_holders_and_truncates_paths() {
        let mut launcher = holder(1234, info("EpicGamesLauncher.exe"));
        launcher.info.image = Some(r"C:\Epic\EpicGamesLauncher.exe".to_string());
        launcher.open_files = (0..7).map(|i| format!(r"D:\f{i}")).collect();
        let system = holder(4, info("System"));
        let text = render(&[launcher.clone(), system], "D:", CONTEXT, false);
        assert!(text.starts_with("2 processes are using D:\n"));
        assert!(text.contains("EpicGamesLauncher.exe (PID 1234)\n  program: C:\\Epic\\EpicGamesLauncher.exe\n  open:    D:\\f0\n"));
        assert!(text.contains(r"D:\f4"));
        assert!(!text.contains(r"D:\f5"));
        assert!(text.contains("... and 2 more"));
        assert!(
            text.contains(
                "System (PID 4)\n  open:    D:\\x\n  cannot be killed: the System process"
            )
        );

        let verbose = render(&[launcher], "D:", CONTEXT, true);
        assert!(verbose.contains(r"D:\f6"));
        assert!(!verbose.contains("more"));
    }
}
