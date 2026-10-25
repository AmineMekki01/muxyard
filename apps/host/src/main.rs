//! Host CLI for version, help, and one-shot local initialization.

use std::{env, process::ExitCode};

const HELP: &str = "Muxyard host scaffold

Usage: muxyard-host [--version | --help | --init]

  --version, -V  Print package and protocol versions
  --help, -h     Print this help
  --init         Initialize per-user configuration, logs, and storage, then exit

The persistent host runtime is not implemented yet. No services are started.";

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [] => println!("{HELP}"),
        [arg] if arg == "--version" || arg == "-V" => {
            println!("{}", muxyard_core::host_version());
        }
        [arg] if arg == "--help" || arg == "-h" => println!("{HELP}"),
        [arg] if arg == "--init" => match muxyard_host::initialize() {
            Ok(bootstrap) => {
                println!(
                    "Local storage initialized (schema {}).",
                    bootstrap.schema_version
                );
            }
            Err(error) => {
                eprintln!("Initialization failed: {error}");
                return ExitCode::from(1);
            }
        },
        _ => {
            eprintln!("Unsupported arguments. Run muxyard-host --help for usage.");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
