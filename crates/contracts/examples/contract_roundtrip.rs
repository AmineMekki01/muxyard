//! Test tool: validate typed JSON from stdin and write its serialized wire form.

use std::{
    env,
    error::Error,
    io::{self, Read},
    process::ExitCode,
};

use muxyard_contracts::{
    AgentStatus, ApiError, ExecutionEnvironment, MAX_CONTROL_BYTES, TaskState, decode_control,
};
use serde::{Serialize, de::DeserializeOwned};

fn roundtrip<T: DeserializeOwned + Serialize>(input: &[u8]) -> Result<(), Box<dyn Error>> {
    match decode_control::<T>(input) {
        Ok(message) => serde_json::to_writer(io::stdout().lock(), &message)?,
        Err(error) => {
            serde_json::to_writer(io::stderr().lock(), &error)?;
            return Err("Contract validation failed.".into());
        }
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    let [kind] = args.as_slice() else {
        return Err("Usage: contract_roundtrip environment|task|agent|error".into());
    };
    if !matches!(kind.as_str(), "environment" | "task" | "agent" | "error") {
        return Err("Unknown fixture kind.".into());
    }
    let mut input = Vec::new();
    io::stdin()
        .lock()
        .take((MAX_CONTROL_BYTES + 1) as u64)
        .read_to_end(&mut input)?;
    match kind.as_str() {
        "environment" => roundtrip::<ExecutionEnvironment>(&input),
        "task" => roundtrip::<TaskState>(&input),
        "agent" => roundtrip::<AgentStatus>(&input),
        "error" => roundtrip::<ApiError>(&input),
        _ => unreachable!("kind was validated before reading stdin"),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("\n{error}");
            ExitCode::FAILURE
        }
    }
}
