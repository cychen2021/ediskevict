//! Scans the volume holding the temp directory and checks that this test process shows up as a holder.

use std::fs::File;

use ediskevict::scan::scan;
use ediskevict::volume::Volume;

#[test]
fn scan_finds_own_open_file_and_executable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("held-by-ediskevict-test.bin");
    let _held = File::create(&path).unwrap();

    let volume = Volume::resolve(&path.to_string_lossy()).unwrap();
    let result = scan(&volume).unwrap();

    let me = result
        .holders
        .iter()
        .find(|h| h.pid == std::process::id())
        .expect("this test process should hold files on the temp volume");
    assert!(
        me.open_files
            .iter()
            .any(|f| f.to_lowercase().ends_with(r"\held-by-ediskevict-test.bin")),
        "open files: {:?}",
        me.open_files
    );

    // The test executable lives under target\ on the repo's volume, which may differ from the temp volume.
    let exe = std::env::current_exe().unwrap();
    if Volume::resolve(&exe.to_string_lossy()).unwrap() == volume {
        let exe_name = exe.file_name().unwrap().to_string_lossy().to_lowercase();
        assert!(
            me.mapped_files
                .iter()
                .any(|f| f.to_lowercase().ends_with(&exe_name)),
            "mapped files: {:?}",
            me.mapped_files
        );
    }
}
