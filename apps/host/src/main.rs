//! Host CLI for the single-instance lifecycle and one-shot initialization.

use std::{env, process::ExitCode};

use muxyard_host::{
    HostError, HostStatus, LockError, StartResult, StopResult, initialize, run_host, start_host,
    status_host, stop_host,
};

const HELP: &str = "Muxyard host

Usage: muxyard-host <command>

Commands:
  run            Run the host in this process until stopped
  start          Start a detached host, or report an existing ready host
  stop           Gracefully stop the current user's host
  status         Show host status; use --json for machine-readable output
  --init         Initialize per-user configuration, logs, and storage, then exit
  --version, -V  Print package and protocol versions
  --help, -h     Print this help

The host runs as your user, stops on logout or reboot, and does not resume
terminals after the host process exits. It never installs a root service.";

fn main() -> ExitCode {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.as_slice() {
        [] => println!("{HELP}"),
        [arg] if arg == "--version" || arg == "-V" => {
            println!("{}", muxyard_core::host_version());
        }
        [arg] if arg == "--help" || arg == "-h" => println!("{HELP}"),
        [arg] if arg == "--init" => return initialize_command(),
        [arg] if arg == "run" => return run_command(),
        [arg] if arg == "start" => return start_command(),
        [arg] if arg == "stop" => return stop_command(),
        [arg] if arg == "status" => return status_command(false),
        [arg, flag] if arg == "status" && flag == "--json" => {
            return status_command(true);
        }
        _ => {
            eprintln!("Unsupported arguments. Run muxyard-host --help for usage.");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

fn initialize_command() -> ExitCode {
    match initialize() {
        Ok(bootstrap) => {
            println!(
                "Local storage initialized (schema {}).",
                bootstrap.schema_version
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Initialization failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn run_command() -> ExitCode {
    match run_host() {
        Ok(()) => ExitCode::SUCCESS,
        Err(HostError::Lock(LockError::AlreadyRunning(info))) => {
            println!("Host already running (pid {}).", info.pid);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Host failed: {error}");
            ExitCode::from(1)
        }
    }
}

fn start_command() -> ExitCode {
    match start_host() {
        Ok(StartResult::Started(info)) => {
            println!(
                "Host started (pid {}). It stops on logout or reboot and does not resume terminals.",
                info.pid
            );
            ExitCode::SUCCESS
        }
        Ok(StartResult::AlreadyRunning(info)) => {
            println!("Host already running (pid {}).", info.pid);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Host failed to start: {error}");
            ExitCode::from(1)
        }
    }
}

fn stop_command() -> ExitCode {
    match stop_host() {
        Ok(StopResult::Stopped(info)) => {
            println!("Host stopped (pid {}).", info.pid);
            ExitCode::SUCCESS
        }
        Ok(StopResult::NotRunning) => {
            println!("Host is not running.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Host failed to stop: {error}");
            ExitCode::from(1)
        }
    }
}

fn status_command(json: bool) -> ExitCode {
    match status_host() {
        Ok(HostStatus::Running { info, ready }) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "running": true,
                        "ready": ready,
                        "pid": info.pid,
                        "instance_id": info.instance_id,
                        "process_start": info.process_start,
                        "started_ms": info.started_ms,
                        "host_version": info.host_version,
                        "protocol_version": info.protocol_version,
                    })
                );
            } else {
                println!(
                    "Host running (pid {}, ready={}, started_ms={}).",
                    info.pid, ready, info.started_ms
                );
            }
            ExitCode::SUCCESS
        }
        Ok(HostStatus::Stale(info)) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "running": false,
                        "stale": true,
                        "pid": info.pid,
                        "instance_id": info.instance_id,
                    })
                );
            } else {
                eprintln!("Host is not running (stale lock for pid {}).", info.pid);
            }
            ExitCode::from(1)
        }
        Ok(HostStatus::NotRunning) => {
            if json {
                println!("{}", serde_json::json!({ "running": false }));
            } else {
                println!("Host is not running.");
            }
            ExitCode::from(1)
        }
        Err(error) => {
            eprintln!("Host status failed: {error}");
            ExitCode::from(1)
        }
    }
}
