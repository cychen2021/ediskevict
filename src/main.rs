use std::io;
use std::io::BufRead;
use std::io::Write;

use anyhow::Result;
use anyhow::bail;
use clap::Parser;
use ediskevict::report::render;
use ediskevict::scan::scan;
use ediskevict::volume::Volume;
use ediskevict::win::process;

/// List the processes that keep a disk busy ("This device is currently in use") and optionally end them, so the disk can be ejected.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// The volume: a drive letter (D:), an absolute path on it, or a \\?\Volume{GUID}\ path
    volume: String,

    /// Offer to end each process found (asks for confirmation unless --yes is given)
    #[arg(long)]
    kill: bool,

    /// With --kill, end the processes without asking
    #[arg(long, short = 'y', requires = "kill")]
    yes: bool,

    /// List every file held, not just the first few per process
    #[arg(long, short)]
    verbose: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let volume = Volume::resolve(&cli.volume)?;
    let label = volume.label();
    if cli.kill && volume.is_system()? {
        bail!(
            "refusing --kill on {label}: Windows is installed there, so it would end most of the system"
        );
    }

    let scan = scan(&volume)?;
    print!(
        "{}",
        render(&scan.holders, &label, scan.context, cli.verbose)
    );
    if !scan.elevated {
        eprintln!();
        eprintln!(
            "warning: not running as Administrator; {} processes could not be inspected, so other users' processes and services may be missing. Run from an elevated terminal for a complete list.",
            scan.inaccessible
        );
    }
    if scan.timed_out > 0 {
        eprintln!(
            "note: skipped {} handles whose names could not be read in time",
            scan.timed_out
        );
    }
    if scan.holders.is_empty() {
        if scan.elevated {
            println!(
                "If Windows still refuses to eject {label}, a driver or Windows itself may be using it (for example a page file, indexing, or antivirus scanning)."
            );
        }
        return Ok(());
    }
    if !cli.kill {
        println!();
        println!("Run again with --kill to end these processes.");
        return Ok(());
    }

    println!();
    let mut killed = 0;
    let mut stdin = io::stdin().lock();
    for holder in &scan.holders {
        let name = &holder.info.name;
        let pid = holder.pid;
        if let Some(reason) = holder.kill_refusal(scan.context) {
            println!("Skipping {name} (PID {pid}): {reason}");
            continue;
        }
        if !cli.yes && !confirm(&mut stdin, &format!("End {name} (PID {pid})? [y/N] "))? {
            continue;
        }
        match process::terminate(pid) {
            Ok(()) => {
                println!("Ended {name} (PID {pid})");
                killed += 1;
            }
            Err(err) => eprintln!("error: {err:#}"),
        }
    }
    if killed > 0 {
        println!();
        println!("Try ejecting {label} again.");
    }
    Ok(())
}

/// Asks a yes/no question on stdin. End of input counts as "no".
fn confirm(stdin: &mut impl BufRead, question: &str) -> Result<bool> {
    print!("{question}");
    io::stdout().flush()?;
    let mut answer = String::new();
    if stdin.read_line(&mut answer)? == 0 {
        println!();
        return Ok(false);
    }
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}
