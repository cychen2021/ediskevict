# ediskevict

Find out which processes keep an external disk busy on Windows, and optionally end them, so the disk can be ejected.

When "Safely Remove Hardware" fails with *"This device is currently in use. Close any programs or windows that might be using the device, and then try again."*, Windows does not say which program is to blame. `ediskevict` does.

## Usage

```
ediskevict <VOLUME> [--kill] [--yes] [--verbose]
```

- `VOLUME`: a drive letter (`D:`), any absolute path on the volume (`D:\Games`), or a volume GUID path (`\\?\Volume{...}\`).
- `--kill`: offer to end each process found, asking `[y/N]` for each one.
- `--yes`, `-y`: with `--kill`, end the processes without asking.
- `--verbose`, `-v`: list every file held, not just the first five per category.

Run it from an **Administrator** terminal. Otherwise other users' processes and Windows services are invisible, and the tool warns how many processes it could not inspect.

Example output (illustrative):

```
> ediskevict D:
1 process is using D:

EpicGamesLauncher.exe (PID 12345)
  program: C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe
  open:    D:\Epic Games\Fortnite\FortniteGame\Content\Paks\global.utoc

Run again with --kill to end these processes.
```

## What it detects

- Open file and directory handles on the volume, including a process's current working directory.
- Executables, DLLs and memory-mapped files loaded from the volume.

## What it will not kill

- Itself, or the shell that started it (`cd` off the drive instead).
- The System process and critical Windows processes.
- Processes hosting Windows services: it suggests `sc stop <service>` instead.
- Anything on the volume Windows is installed on: `--kill` is refused there.

If nothing is found but Windows still refuses to eject, the holder is likely a driver or Windows itself (page file, indexing, antivirus scanning).

## Building

```
cargo build --release
cargo test
```
