//! Checks that `terminate` only ends the process whose start time it was given.

use std::process::Command;
use std::process::Stdio;

use ediskevict::win::process;

#[test]
fn terminate_refuses_mismatched_start_time() {
    let mut child = Command::new("ping")
        .args(["-n", "60", "127.0.0.1"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let started = process::start_time(pid).expect("start time of a child process");

    // A different start time stands in for a PID reused by another process after the scan.
    let err = process::terminate(pid, started + 1).unwrap_err();
    assert!(
        format!("{err:#}").contains("now belongs to another process"),
        "{err:#}"
    );
    assert!(
        child.try_wait().unwrap().is_none(),
        "the mismatched process must survive"
    );

    process::terminate(pid, started).unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(1));
}
