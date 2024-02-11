use crate::assert_matches_template;
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
use std::{
    net::SocketAddr,
    path::Path,
    process::{Command, Output, Stdio},
    str::FromStr,
    time::Duration,
};

fn bin_command() -> Command {
    const BIN_EXE: &str = env!("CARGO_BIN_EXE_zpool-status-exporter");
    const BIN_EXE_ZPOOL: &str = env!("CARGO_BIN_EXE_zpool");

    let mut command = Command::new(BIN_EXE);

    {
        // Overwrite path to the fake zpool executable (usually target debug dir)
        // (forbid use of system's `zpool` command)
        let path_to_bin_exe_zpool = Path::new(BIN_EXE_ZPOOL)
            .parent()
            .expect("absolute path in zpool CARGO_BIN_EXE");

        command.env("PATH", path_to_bin_exe_zpool);
    }

    command
}

#[test]
fn run_bin() -> anyhow::Result<()> {
    const EXPECTED_METRICS_OUTPUT: &str = include_str!("../../src/bin/output-integration.txt");

    const LISTEN_ADDRESS_STR: &str = "127.0.0.1:9583";
    let listen_address = SocketAddr::from_str(LISTEN_ADDRESS_STR)?;

    // startup server
    let mut subcommand = bin_command()
        .arg(LISTEN_ADDRESS_STR)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // allow grace period for init
    std::thread::sleep(Duration::from_millis(100));

    // request from `/metrics` endpoint
    let response_metrics = minreq::get(format!("http://{LISTEN_ADDRESS_STR}/metrics")).send();

    // request non-existent URL
    let response_unknown = minreq::get(format!("http://{LISTEN_ADDRESS_STR}/")).send();

    // SIGINT - request clean exit
    signal::kill(
        Pid::from_raw(subcommand.id().try_into().unwrap()),
        Signal::SIGINT,
    )?;

    // allow grace period for cleanup
    std::thread::sleep(Duration::from_millis(300));
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

    // no fatal errors
    //
    // "NOTSURE?" is mentioned twice:
    // 1. once for fail-fast startup run, and
    // 2. again for the "/metrics" request
    assert_eq!(
        stderr,
        "Unrecognized DeviceStatus: \"NOTSURE?\"\nUnrecognized DeviceStatus: \"NOTSURE?\"\n",
        "stderr"
    );
    assert_eq!(
        stdout,
        format!("Listening at http://{listen_address}\n"),
        "stdout"
    );
    assert!(
        status.success(),
        "verify sleep duration after SIGINT, killing too early?"
    );

    // ----

    let response_metrics = response_metrics?;
    let response_metrics_status = response_metrics.status_code;
    let response_metrics = response_metrics.as_str()?;

    let response_unknown = response_unknown?;
    let response_unknown_status = response_unknown.status_code;
    let response_unknown = response_unknown.as_str()?;

    assert_eq!(response_unknown, "", "response_unknown");
    assert_eq!(response_unknown_status, 404);

    assert_matches_template(response_metrics, EXPECTED_METRICS_OUTPUT);
    assert_eq!(response_metrics_status, 200);

    Ok(())
}
