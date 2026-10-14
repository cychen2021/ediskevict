# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

# ediskevict

A Windows command-line tool, written in Rust, that lists the processes keeping an external disk busy and can optionally kill them, so the disk can be safely ejected.

## Motivation

The problem it solves: after installing Epic Games titles on an external SSD (`D:\`), "Safely Remove Hardware" fails with *"Problem Ejecting USB Attached SCSI (UAS) Mass Storage Device — This device is currently in use. Close any programs or windows that might be using the device, and then try again."* (see `x_screenshot.png`). Windows does not say *which* program is using the device; this tool finds out.

## Status

The project is a freshly initialized Cargo binary crate (`src/main.rs` is still the hello-world stub, with `anyhow` and `clap` already added as dependencies). Follow the conventions below (taken from the author's [vischat](https://github.com/cychen2021/vischat) project) and update this file as the architecture takes shape. `AGENTS.md` mirrors this file, so keep the two identical. Shared skills live in the `.claude/skills` git submodule (`cychen2021/claude-skills`); run `git submodule update --init` after cloning.

## Commands

```bash
cargo build                      # build
cargo run -- <args>              # run, e.g. `cargo run -- D:`
cargo test                       # all tests
cargo test <name_substring>      # tests whose name contains the substring
cargo test --test <file_stem>    # one integration test file under tests/
cargo clippy --all-targets       # lint
cargo fmt                        # format
cargo insta test / cargo insta review   # snapshot tests, if insta is used for output rendering
```

Enumerating handles that belong to other users' processes, and killing them, usually needs an elevated (Administrator) terminal. Expect partial results when not elevated, and report that to the user instead of failing silently.

## Design notes

- Windows only. "In use" can mean several things, and a complete answer should cover all of them for a given volume:
  - Open file or directory handles whose path is on the volume. This is the usual cause. Enumerate system handles (`NtQuerySystemInformation` with `SystemExtendedHandleInformation`, then duplicate each handle and `NtQueryObject`/`GetFinalPathNameByHandleW`) and match path prefixes against the volume. Querying the name of some handle types (for example synchronous named pipes) can block forever, so do it with a timeout or skip non-file handle types.
  - Processes whose executable image or loaded DLLs live on the volume (for example the game itself, or launcher helpers).
  - Processes whose current working directory is on the volume.
- Restart Manager (`RmGetList`) only takes individual file paths, not a whole volume. It is useful for checking known files, but it cannot replace handle enumeration.
- Match volumes robustly: a drive letter, a `\\?\Volume{GUID}\` path and an NT device path (`\Device\HarddiskVolumeN\...`) can all refer to the same disk, and one physical disk can hold several volumes.
- Killing must be opt-in, and the tool should confirm before it kills anything. Some holders cannot or should not be killed (`System` PID 4, services such as `SearchIndexer.exe` or antivirus). Report these clearly, and suggest an alternative such as stopping the service, instead of attempting the kill.
- Likely Win32 binding crates are `windows` or `windows-sys`. Keep `unsafe` FFI behind a small safe module so that the CLI and output layers stay safe Rust.

## Conventions (from vischat)

- Error handling: `anyhow` with `Result<T>`. CLI parsing: `clap` with the `derive` feature. Rust edition 2024.
- Put unit tests in the same file as the code, and integration and CLI tests in `tests/`.
- Prefer a `use` import to an absolute path at the point of use. Write `use crate::foo::Bar;` and then `Bar`, not `crate::foo::Bar` inline.
- Keep each `use` group to a single brace level. A flat list is fine (`use crate::model::{A, B};`), but put nested paths on their own lines (`use ratatui::Terminal;` and `use ratatui::backend::CrosstermBackend;`, not `use ratatui::{Terminal, backend::CrosstermBackend};`).
- Add doc comments to public APIs.
- Do not hard-wrap lines in Markdown docs (README.md, this file). Write each paragraph or list item on one line.
- `x_*` and `tmp/` are scratch paths and should be gitignored (as `x_screenshot.png` is), so don't commit them.
- Commit messages: in English. The headline has fewer than 10 words, starts with a capitalized verb and has no trailing period. Details go in a bullet list after one blank line. Wrap code items and file paths in backticks. Put the most important change first. Avoid vague verbs ("refactor", "enhance", "improve") when a more precise one exists. Describe moves and renames as moves and renames, not as a deletion plus an addition. A minor change gets a one-line message.
