//! Native entry point for the original Muxyard desktop shell.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    if let Err(error) = muxyard_desktop::run() {
        eprintln!("Muxyard desktop failed to start: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
