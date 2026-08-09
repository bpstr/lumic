#![cfg(target_os = "linux")]

use std::{process::Command, thread, time::Duration};

#[test]
fn exits_cleanly_on_sigterm() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lumicd"))
        .env("RUST_LOG", "info")
        .spawn()
        .expect("start lumicd");
    thread::sleep(Duration::from_millis(150));

    // SAFETY: the PID belongs to the child process spawned above and SIGTERM is valid.
    let signal_result = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    assert_eq!(signal_result, 0, "send SIGTERM to lumicd");

    let status = child.wait().expect("wait for lumicd");
    assert!(status.success(), "lumicd should stop cleanly: {status}");
}
