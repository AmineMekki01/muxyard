//! Exercise the real host executable, including failure and repeated invocation.

use std::process::Command;

fn host() -> Command {
    Command::new(env!("CARGO_BIN_EXE_muxyard-host"))
}

#[test]
fn version_reports_semver_and_protocol_on_repeated_starts() {
    for _ in 0..2 {
        let output = host().arg("--version").output().expect("host should start");
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).expect("UTF-8 version"),
            format!("muxyard-host {} (protocol 1)\n", env!("CARGO_PKG_VERSION"))
        );
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn no_arguments_and_help_disclose_that_no_services_start() {
    for args in [vec![], vec!["--help"], vec!["-h"]] {
        let output = host().args(args).output().expect("host should start");
        assert!(output.status.success());
        assert!(
            String::from_utf8(output.stdout)
                .expect("UTF-8 help")
                .contains("No services are started.")
        );
    }
}

#[test]
fn unsupported_or_extra_arguments_fail_without_starting_services() {
    for args in [vec!["--serve"], vec!["--version", "--serve"]] {
        let output = host().args(args).output().expect("host should start");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .expect("UTF-8 diagnostic")
                .contains("Run muxyard-host --help")
        );
    }
}
