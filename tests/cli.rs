//! Runs the `ediskevict` binary end to end.

use std::process::Command;
use std::process::Output;
use std::process::Stdio;

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ediskevict"))
        .args(args)
        // Any confirmation prompt then reads end of input, which counts as "no".
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn help_describes_options() {
    let output = run(&["--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for option in ["--kill", "--yes", "--verbose"] {
        assert!(stdout.contains(option), "{option} missing from:\n{stdout}");
    }
}

#[test]
fn rejects_unrecognized_volume() {
    let output = run(&["not-a-volume"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unrecognized volume"));
}

#[test]
fn yes_requires_kill() {
    let output = run(&["C:", "--yes"]);
    assert!(!output.status.success());
}

#[test]
fn refuses_kill_on_system_volume() {
    let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
    let output = run(&[&system_drive, "--kill"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("refusing --kill"), "stderr:\n{stderr}");
}
