//! Host CLI scaffold. FND-001 starts no daemon, child process, or listener.

use std::{env, process::ExitCode};

const HELP: &str = "Muxyard host scaffold

Usage: muxyard-host [--version | --help]

  --version, -V  Print package and protocol versions
  --help, -h     Print this help

The persistent host runtime is not implemented yet. No services are started.";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [] => println!("{HELP}"),
        [arg] if arg == "--version" || arg == "-V" => {
            println!("{}", muxyard_core::host_version());
        }
        [arg] if arg == "--help" || arg == "-h" => println!("{HELP}"),
        _ => {
            eprintln!("Unsupported arguments. Run muxyard-host --help for usage.");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
