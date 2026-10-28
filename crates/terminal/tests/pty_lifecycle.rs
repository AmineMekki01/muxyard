//! Real PTY lifecycle tests for Unix shells.

#![cfg(unix)]

use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use muxyard_terminal::{
    PtySession, PtySessionConfig, TerminalHub, TerminalLimits, TerminalSubscription,
};

fn hub() -> Arc<TerminalHub> {
    Arc::new(TerminalHub::new(TerminalLimits::default()).unwrap())
}

fn config(cwd: PathBuf) -> PtySessionConfig {
    PtySessionConfig {
        shell: Some(PathBuf::from("/bin/sh")),
        cwd,
        rows: 24,
        cols: 80,
        environment: vec![("TERM".into(), "xterm-256color".into())],
        ..PtySessionConfig::default()
    }
}

fn spawn(hub: &Arc<TerminalHub>, cwd: PathBuf) -> PtySession {
    PtySession::spawn(config(cwd), Arc::clone(hub)).unwrap()
}

fn subscribe(hub: &Arc<TerminalHub>, session: &PtySession) -> TerminalSubscription {
    hub.subscribe(session.stream_id(), None).unwrap()
}

fn read_until(subscription: &TerminalSubscription, needle: &[u8], timeout: Duration) -> Vec<u8> {
    let deadline = Instant::now() + timeout;
    let mut output = Vec::new();
    while Instant::now() < deadline {
        if let Some(frame) = subscription.wait_next(Duration::from_millis(50)) {
            output.extend_from_slice(frame.payload());
            if output.windows(needle.len()).any(|window| window == needle) {
                return output;
            }
        }
        if subscription.is_detached() {
            break;
        }
    }
    output
}

#[test]
fn shell_echo_and_cwd_are_visible() {
    let temp = tempfile::tempdir().unwrap();
    let hub = hub();
    let mut session = spawn(&hub, temp.path().to_path_buf());
    let identity = session.identity();
    assert_eq!(identity.stream_id, session.stream_id());
    assert!(identity.cwd.is_absolute());
    assert!(identity.shell.is_absolute());
    let subscription = subscribe(&hub, &session);

    session.write(b"printf 'hello-pty\\n'\n").unwrap();
    let output = read_until(&subscription, b"hello-pty", Duration::from_secs(5));
    assert!(
        String::from_utf8_lossy(&output).contains("hello-pty"),
        "missing echo output: {:?}",
        String::from_utf8_lossy(&output)
    );

    session.write(b"pwd\n").unwrap();
    let expected = temp.path().canonicalize().unwrap();
    let output = read_until(
        &subscription,
        expected.to_string_lossy().as_bytes(),
        Duration::from_secs(5),
    );
    assert!(
        String::from_utf8_lossy(&output).contains(expected.to_string_lossy().as_ref()),
        "missing cwd output: {:?}",
        String::from_utf8_lossy(&output)
    );

    session.close().unwrap();
    assert_eq!(hub.session_count(), 0);
}

#[test]
fn snapshot_respects_configured_tail_bound() {
    let temp = tempfile::tempdir().unwrap();
    let hub = hub();
    let mut config = config(temp.path().to_path_buf());
    config.output_tail_bytes = 64;
    let mut session = PtySession::spawn(config, Arc::clone(&hub)).unwrap();
    let subscription = subscribe(&hub, &session);

    session
        .write(b"printf '%0256d' 0 | tr '0' 'x'; printf '\\n'\n")
        .unwrap();
    read_until(&subscription, b"xxxx", Duration::from_secs(5));
    let snapshot = session.snapshot();
    assert!(!snapshot.is_empty());
    assert!(
        snapshot.len() <= 64,
        "tail grew to {} bytes",
        snapshot.len()
    );

    session.close().unwrap();
}

#[test]
fn large_output_keeps_the_configured_tail_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let hub = hub();
    let mut config = config(temp.path().to_path_buf());
    config.output_tail_bytes = 64 * 1024;
    let mut session = PtySession::spawn(config, Arc::clone(&hub)).unwrap();
    let subscription = hub
        .subscribe(session.stream_id(), Some(4 * 1024 * 1024))
        .unwrap();

    session
        .write(b"head -c 600000 /dev/zero | tr '\\0' 'x'; printf '\\nDONE-%s\\n' \"$((1+1))\"\n")
        .unwrap();
    let output = read_until(&subscription, b"DONE-2", Duration::from_secs(10));
    assert!(
        output.len() > 500_000,
        "fixture output was too small: {} bytes",
        output.len()
    );

    let snapshot = session.snapshot();
    assert!(!snapshot.is_empty());
    assert!(
        snapshot.len() <= 64 * 1024,
        "large-output snapshot grew to {} bytes",
        snapshot.len()
    );
    assert!(snapshot.contains(&b'x'), "newest output was not retained");

    session.close().unwrap();
}

#[test]
fn shell_receives_terminal_color_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let hub = hub();
    let mut session = spawn(&hub, temp.path().to_path_buf());
    let subscription = subscribe(&hub, &session);

    session
        .write(
            br#"printf '%s|%s|%s|%s|%s\n' "$CLICOLOR" "$COLORTERM" "$TERM_PROGRAM" "$LSCOLORS" "$LS_COLORS"
"#,
        )
        .unwrap();
    let output = read_until(
        &subscription,
        b"1|truecolor|Muxyard|ExGxFxDxCxDxDxBxBxExEx|",
        Duration::from_secs(5),
    );
    let text = String::from_utf8_lossy(&output);
    assert!(
        text.contains("1|truecolor|Muxyard|ExGxFxDxCxDxDxBxBxExEx|"),
        "missing terminal color environment: {text:?}"
    );
    session.close().unwrap();
}

#[test]
fn ls_output_contains_color_sequences() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("folder")).unwrap();
    fs::write(temp.path().join("file.txt"), b"x").unwrap();
    let hub = hub();
    let mut session = spawn(&hub, temp.path().to_path_buf());
    let subscription = subscribe(&hub, &session);

    // GNU ls only emits colors when asked; BSD/macOS ls honors CLICOLOR_FORCE.
    #[cfg(target_os = "linux")]
    session.write(b"ls --color=always\n").unwrap();
    #[cfg(not(target_os = "linux"))]
    session.write(b"ls\n").unwrap();
    let output = read_until(&subscription, b"file.txt", Duration::from_secs(5));
    assert!(
        output.windows(2).any(|window| window == b"\x1b["),
        "ls output did not contain ANSI color sequences: {:?}",
        String::from_utf8_lossy(&output)
    );
    session.close().unwrap();
}

#[test]
fn resize_and_exit_status_are_observed() {
    let temp = tempfile::tempdir().unwrap();
    let hub = hub();
    let mut session = spawn(&hub, temp.path().to_path_buf());
    assert_eq!(session.size().unwrap(), (24, 80));
    session.resize(31, 101).unwrap();
    assert_eq!(session.size().unwrap(), (31, 101));

    session.write(b"exit 7\n").unwrap();
    let status = session.wait_for_exit(Duration::from_secs(5)).unwrap();
    assert_eq!(status.code, 7);
    assert!(status.signal.is_none());
    session.close().unwrap();
}

#[test]
fn ctrl_c_interrupts_a_running_command() {
    let temp = tempfile::tempdir().unwrap();
    let hub = hub();
    let mut session = spawn(&hub, temp.path().to_path_buf());
    let subscription = subscribe(&hub, &session);

    session.write(b"sleep 30\n").unwrap();
    thread::sleep(Duration::from_millis(200));
    session.write(b"\x03").unwrap();
    thread::sleep(Duration::from_millis(100));
    session.write(b"printf 'after-interrupt\\n'\n").unwrap();
    let output = read_until(&subscription, b"after-interrupt", Duration::from_secs(5));
    assert!(
        String::from_utf8_lossy(&output).contains("after-interrupt"),
        "shell did not recover after Ctrl+C: {:?}",
        String::from_utf8_lossy(&output)
    );
    session.close().unwrap();
}

#[test]
fn four_sessions_are_independent() {
    let temp = tempfile::tempdir().unwrap();
    let hub = hub();
    let mut sessions = Vec::new();
    let mut subscriptions = Vec::new();
    for index in 0..4 {
        let mut session = spawn(&hub, temp.path().to_path_buf());
        let subscription = subscribe(&hub, &session);
        session
            .write(format!("printf 'session-{index}\\n'\n").as_bytes())
            .unwrap();
        sessions.push(session);
        subscriptions.push(subscription);
    }

    for (index, subscription) in subscriptions.iter().enumerate() {
        let marker = format!("session-{index}");
        let output = read_until(subscription, marker.as_bytes(), Duration::from_secs(5));
        assert!(
            String::from_utf8_lossy(&output).contains(&marker),
            "missing marker for session {index}: {:?}",
            String::from_utf8_lossy(&output)
        );
    }

    for session in sessions {
        session.close().unwrap();
    }
    assert_eq!(hub.session_count(), 0);
}

#[test]
fn shell_path_with_spaces_is_launched_without_interpolation() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let shell_path = temp.path().join("shell with spaces");
    std::fs::write(&shell_path, b"#!/bin/sh\nexec /bin/sh\n").unwrap();
    std::fs::set_permissions(&shell_path, std::fs::Permissions::from_mode(0o700)).unwrap();

    let hub = hub();
    let mut session = PtySession::spawn(
        PtySessionConfig {
            shell: Some(shell_path),
            cwd: temp.path().to_path_buf(),
            rows: 24,
            cols: 80,
            environment: Vec::new(),
            ..PtySessionConfig::default()
        },
        Arc::clone(&hub),
    )
    .unwrap();
    let subscription = subscribe(&hub, &session);
    session.write(b"printf 'spaces-ok\\n'\n").unwrap();
    let output = read_until(&subscription, b"spaces-ok", Duration::from_secs(5));
    assert!(String::from_utf8_lossy(&output).contains("spaces-ok"));
    session.close().unwrap();
}

#[test]
fn large_unicode_stream_is_forwarded() {
    let temp = tempfile::tempdir().unwrap();
    let hub = hub();
    let mut session = spawn(&hub, temp.path().to_path_buf());
    let subscription = subscribe(&hub, &session);
    session.write(b"stty -icanon -echo\n").unwrap();
    thread::sleep(Duration::from_millis(100));
    session.write(b"cat\n").unwrap();
    thread::sleep(Duration::from_millis(100));

    let payload = "世界-🙂-".repeat(8192);
    let marker = "END-世界";
    session.write(payload.as_bytes()).unwrap();
    session.write(marker.as_bytes()).unwrap();
    let output = read_until(&subscription, marker.as_bytes(), Duration::from_secs(5));
    assert!(output.len() >= payload.len());
    assert!(
        String::from_utf8_lossy(&output).contains(marker),
        "large unicode output was not forwarded"
    );
    session.write(b"\x03").unwrap();
    thread::sleep(Duration::from_millis(100));
    session.write(b"exit\n").unwrap();
    session.close().unwrap();
}

#[test]
fn restart_after_close_starts_a_fresh_session() {
    let temp = tempfile::tempdir().unwrap();
    let hub = hub();
    let first = spawn(&hub, temp.path().to_path_buf());
    let first_id = first.stream_id();
    first.close().unwrap();

    let mut second = spawn(&hub, temp.path().to_path_buf());
    assert_ne!(first_id, second.stream_id());
    let subscription = subscribe(&hub, &second);
    second.write(b"printf 'restart-ok\\n'\n").unwrap();
    let output = read_until(&subscription, b"restart-ok", Duration::from_secs(5));
    assert!(String::from_utf8_lossy(&output).contains("restart-ok"));
    second.close().unwrap();
    assert_eq!(hub.session_count(), 0);
}

#[test]
fn missing_shell_is_rejected_before_stream_creation() {
    let hub = hub();
    let error = PtySession::spawn(
        PtySessionConfig {
            shell: Some(PathBuf::from("/muxyard-test/missing-shell")),
            cwd: std::env::current_dir().unwrap(),
            rows: 24,
            cols: 80,
            environment: Vec::new(),
            ..PtySessionConfig::default()
        },
        Arc::clone(&hub),
    )
    .err()
    .expect("missing shell must fail");
    assert!(matches!(error, muxyard_terminal::PtyError::Platform(_)));
    assert_eq!(hub.session_count(), 0);
}

#[test]
fn invalid_cwd_and_forced_death_are_handled() {
    let hub = hub();
    let error = PtySession::spawn(
        config(PathBuf::from("/muxyard-test/missing-cwd")),
        Arc::clone(&hub),
    )
    .err()
    .expect("missing cwd must fail");
    assert!(matches!(error, muxyard_terminal::PtyError::Platform(_)));
    assert_eq!(hub.session_count(), 0);

    let temp = tempfile::tempdir().unwrap();
    let mut session = spawn(&hub, temp.path().to_path_buf());
    session.kill().unwrap();
    let status = session.wait_for_exit(Duration::from_secs(5));
    assert!(status.is_some());
    session.close().unwrap();
    assert_eq!(hub.session_count(), 0);
}

#[test]
fn large_unicode_input_roundtrips() {
    let temp = tempfile::tempdir().unwrap();
    let hub = hub();
    let mut session = spawn(&hub, temp.path().to_path_buf());
    let subscription = subscribe(&hub, &session);
    let text = "héllo-世界-🙂";
    session
        .write(format!("printf '%s\\n' '{text}'\n").as_bytes())
        .unwrap();
    let output = read_until(&subscription, text.as_bytes(), Duration::from_secs(5));
    assert!(
        String::from_utf8_lossy(&output).contains(text),
        "missing unicode output: {:?}",
        String::from_utf8_lossy(&output)
    );
    session.close().unwrap();
}
