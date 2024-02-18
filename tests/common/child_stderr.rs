use crate::{
    bin_cmd::{BinCommand, BinOutput, FakeZpoolMode},
    HTTP_OK,
};
use std::{net::SocketAddr, str::FromStr};

#[test]
fn stderr_no_pools() -> anyhow::Result<()> {
    const LISTEN_ADDRESS: &str = crate::common::LISTEN_ADDRESS_CHILD_STDERR_1;

    let listen_address = SocketAddr::from_str(LISTEN_ADDRESS)?;

    let (output, response_metrics) = BinCommand::new()
        .arg(LISTEN_ADDRESS)
        .fake_zpool_mode(FakeZpoolMode::NoPools)
        .spawn_cleanup_with(|| {
            minreq::get(format!("http://{listen_address}/metrics")).send() //
        })?;

    {
        let BinOutput {
            status,
            stdout: _,
            stderr,
        } = output;

        // assert_eq!(stdout, "");
        assert_eq!(stderr, "user requested shutdown...\n");
        assert!(status.success());
    }

    {
        let response_metrics = response_metrics?;

        let response_metrics_status = response_metrics.status_code;
        let response_metrics = response_metrics.as_str()?;

        assert_eq!(response_metrics_status, HTTP_OK);

        let mut lines = response_metrics.lines();
        let line = lines.next().expect("has line 1");
        assert!(
            line.starts_with("# total duration of the lookup"),
            "line 1 {line:?}",
        );
        let line = lines.next().expect("has line 2");
        assert!(
            line.starts_with("zpool_status_export_lookup"),
            "line 2 {line:?}"
        );
        assert_eq!(lines.next(), None, "no extra lines");
    }

    Ok(())
}

#[test]
fn stderr_devs_missing() -> anyhow::Result<()> {
    const LISTEN_ADDRESS: &str = crate::common::LISTEN_ADDRESS_CHILD_STDERR_2;

    let (output, ()) = BinCommand::new()
        .arg(LISTEN_ADDRESS)
        .fake_zpool_mode(FakeZpoolMode::DevsMissing)
        .spawn_cleanup_with(|| {})?;

    {
        let BinOutput {
            status,
            stdout,
            stderr,
        } = output;

        assert_eq!(stdout, "");
        assert!(
            stderr.contains("zpool requires access to /dev/zfs and /proc/self/mounts"),
            "stderr {stderr:?}"
        );
        assert!(!status.success());
    }

    Ok(())
}
