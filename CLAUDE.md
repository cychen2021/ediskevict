# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# ediskevict

A Windows command-line tool, written in Rust, that lists the processes keeping an external disk busy and can optionally kill them, so the disk can be safely ejected.

## Motivation

The problem it solves: after installing Epic Games titles on an external SSD (`D:\`), "Safely Remove Hardware" fails with *"Problem Ejecting USB Attached SCSI (UAS) Mass Storage Device — This device is currently in use. Close any programs or windows that might be using the device, and then try again."* (see `x_screenshot.png`). Windows does not say *which* program is using the device; this tool finds out.

## Status

The tool is implemented: `ediskevict <VOLUME> [--kill] [--yes] [--verbose]` (see README.md). Follow the conventions below (taken from the author's [vischat](https://github.com/cychen2021/vischat) project). `AGENTS.md` is a symlink to this file. Shared skills live in the `.claude/skills` git submodule (`cychen2021/claude-skills`); run `git submodule update --init` after cloning.

## Commands

```bash
cargo build                      # build
cargo run -- <args>              # run, e.g. `cargo run -- D:`
cargo test                       # all tests
cargo test <name_substring>      # tests whose name contains the substring
cargo test --test <file_stem>    # one integration test file: `scan` (scans the temp dir's volume) or `cli`
cargo clippy --all-targets       # lint
cargo fmt                        # format
```

Enumerating handles that belong to other users' processes, and killing them, usually needs an elevated (Administrator) terminal. Expect partial results when not elevated, and report that to the user instead of failing silently.

## Architecture

The pipeline is `main.rs` → `Volume::resolve` → `scan::scan` → `report::group`/`render` → optional `process::terminate`. All `unsafe` FFI (`windows-sys`) is confined to `src/win/`; everything above it is safe Rust, and the pure logic (`volume.rs` matching, `report.rs` grouping, kill refusals and rendering) is unit-tested without touching Windows.

- `volume.rs`: everything is matched on the NT device name (`\Device\HarddiskVolumeN`), because that is the form both handle paths (`GetFinalPathNameByHandleW` with `VOLUME_NAME_NT`) and memory mappings (`GetMappedFileNameW`) come back in. A user spec (drive letter, path, or `\\?\Volume{GUID}\`) is resolved spec → mount point → GUID path → device; `display_path` maps device paths back to the first mount point.
- `scan.rs` combines two sources, which together cover the three ways a volume is "in use":
  - `win/handles.rs`: open file and directory handles (which include each process's working directory). It reads the system handle table (`NtQuerySystemInformation`, class 64; the entry struct is defined locally because `windows-sys` lacks it). The "File" object type index varies by Windows build, so it is learned by finding a handle this process opened to its own exe. Each candidate handle is duplicated and named on a worker thread with a 500 ms timeout, because name queries on synchronous pipes can block forever; on timeout the worker and the duplicated handle are deliberately leaked, never closed.
  - `win/memory.rs`: files mapped into memory (exe, DLLs, mapped data files), via `VirtualQueryEx` + `GetMappedFileNameW`. The loader closes its file handles, so handle enumeration alone misses these.
- `report.rs`: `Holder::kill_refusal` is the single place that decides what must never be killed: this process, its parent shell, PIDs ≤ 4, critical processes (`IsProcessCritical`), and service hosts (suggesting `sc stop` instead). `main.rs` additionally refuses `--kill` on the system volume before scanning, and treats end of stdin as "no" at each prompt.
- When elevated, the scan enables `SeDebugPrivilege`; when not, it reports how many processes it could not inspect instead of failing.
- Restart Manager (`RmGetList`) was rejected: it takes individual file paths, not a whole volume.

## Conventions (from vischat)

- Error handling: `anyhow` with `Result<T>`. CLI parsing: `clap` with the `derive` feature. Rust edition 2024.
- Put unit tests in the same file as the code, and integration and CLI tests in `tests/`.
- Prefer a `use` import to an absolute path at the point of use. Write `use crate::foo::Bar;` and then `Bar`, not `crate::foo::Bar` inline.
- Keep each `use` group to a single brace level. A flat list is fine (`use crate::model::{A, B};`), but put nested paths on their own lines (`use ratatui::Terminal;` and `use ratatui::backend::CrosstermBackend;`, not `use ratatui::{Terminal, backend::CrosstermBackend};`).
- Add doc comments to public APIs.
- Do not hard-wrap lines in Markdown docs (README.md, this file). Write each paragraph or list item on one line.
- `x_*` and `tmp/` are scratch paths and should be gitignored (as `x_screenshot.png` is), so don't commit them.
- Commit messages: in English. The headline has fewer than 10 words, starts with a capitalized verb and has no trailing period. Details go in a bullet list after one blank line. Wrap code items and file paths in backticks. Put the most important change first. Avoid vague verbs ("refactor", "enhance", "improve") when a more precise one exists. Describe moves and renames as moves and renames, not as a deletion plus an addition. A minor change gets a one-line message.
