use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
use std::{
    net::SocketAddr,
    process::{Command, Output, Stdio},
    str::FromStr,
    time::Duration,
};

const BIN_EXE: &str = env!("CARGO_BIN_EXE_zpool-status-exporter");

#[test]
fn run_bin() -> anyhow::Result<()> {
    const LISTEN_ADDRESS_STR: &str = "127.0.0.1:9583";
    let listen_address = SocketAddr::from_str(LISTEN_ADDRESS_STR)?;

    let mut subcommand = Command::new(BIN_EXE)
        .arg(LISTEN_ADDRESS_STR)
        .env_clear()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    std::thread::sleep(Duration::from_millis(100));

    // NOTE: Not kill, to check exit status is "success"
    //     subcommand.kill()?;
    signal::kill(
        Pid::from_raw(subcommand.id().try_into().unwrap()),
        Signal::SIGINT,
    )?;

    // allow grace period for cleanup
    std::thread::sleep(Duration::from_millis(100));
    // but don't wait forever
    subcommand.kill()?;

    let output = subcommand.wait_with_output()?;

    let Output {
        status,
        stdout,
        stderr,
    } = output;
    let stdout = String::from_utf8(stdout)?;
    let stderr = String::from_utf8(stderr)?;

    // no errors
    assert_eq!(stderr, "");
    assert!(status.success());

    assert_eq!(stdout, format!("Listening at {listen_address}\n"));

    Ok(())
}
