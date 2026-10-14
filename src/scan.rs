//! Finds every process holding files on a volume, combining open handles and memory mappings.

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Result;

use crate::report::Context;
use crate::report::Finding;
use crate::report::HoldKind;
use crate::report::Holder;
use crate::report::ProcessInfo;
use crate::report::group;
use crate::volume::Volume;
use crate::win::handles;
use crate::win::memory;
use crate::win::process;

/// The outcome of [`scan`].
#[derive(Debug)]
pub struct Scan {
    pub holders: Vec<Holder>,
    pub context: Context,
    /// Whether the scan ran elevated. Without elevation, other users' processes and services are invisible.
    pub elevated: bool,
    /// Processes that could not be fully inspected.
    pub inaccessible: usize,
    /// Handles skipped because querying their name hung.
    pub timed_out: usize,
}

/// Scans all processes for files held on `volume`.
pub fn scan(volume: &Volume) -> Result<Scan> {
    let elevated = process::is_elevated();
    if elevated {
        process::enable_debug_privilege();
    }
    let processes = process::list()?;
    let self_pid = std::process::id();
    let context = Context {
        self_pid,
        parent_pid: processes
            .iter()
            .find(|p| p.pid == self_pid)
            .map(|p| p.parent_pid),
    };

    let handle_scan = handles::open_files(|path| volume.contains(path))?;
    let mut inaccessible: HashSet<u32> = handle_scan.inaccessible;
    let mut findings: Vec<Finding> = handle_scan
        .files
        .into_iter()
        .map(|(pid, path)| Finding {
            pid,
            kind: HoldKind::OpenHandle,
            path: volume.display_path(&path),
        })
        .collect();

    // PID 0 is the idle pseudo-process, which has no address space.
    for entry in processes.iter().filter(|p| p.pid != 0) {
        match memory::mapped_files(entry.pid) {
            Some(files) => {
                findings.extend(files.into_iter().filter(|path| volume.contains(path)).map(
                    |path| Finding {
                        pid: entry.pid,
                        kind: HoldKind::Mapped,
                        path: volume.display_path(&path),
                    },
                ))
            }
            None => {
                inaccessible.insert(entry.pid);
            }
        }
    }

    let names: HashMap<u32, &str> = processes.iter().map(|p| (p.pid, p.name.as_str())).collect();
    let mut services = process::services_by_pid();
    let holders = group(findings, |pid| ProcessInfo {
        name: names
            .get(&pid)
            .map_or_else(|| "<exited>".to_string(), |n| n.to_string()),
        image: process::image_path(pid),
        services: services.remove(&pid).unwrap_or_default(),
        critical: process::is_critical(pid),
        started: process::start_time(pid),
    });
    Ok(Scan {
        holders,
        context,
        elevated,
        inaccessible: inaccessible.len(),
        timed_out: handle_scan.timed_out,
    })
}
