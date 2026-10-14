//! Finds (and optionally ends) the processes that keep a disk volume busy, so that it can be ejected.

#[cfg(not(windows))]
compile_error!("ediskevict only supports Windows");

pub mod report;
pub mod scan;
pub mod volume;
pub mod win;
