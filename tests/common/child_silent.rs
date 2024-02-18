use crate::common::bin_cmd::{BinCommand, BinOutput, FakeZpoolMode};
use std::time::{Duration, Instant};

#[test]
fn child_silent() -> anyhow::Result<()> {
    const LISTEN_ADDRESS: &str = crate::common::LISTEN_ADDRESS_CHILD_SILENT_1;

    let (output, ()) = BinCommand::new()
        .arg(LISTEN_ADDRESS)
        .fake_zpool_mode(FakeZpoolMode::Silent)
        .spawn_cleanup_with(|| {})?;

    {
        let BinOutput {
            status,
            stdout,
            stderr,
        } = output;

        assert_eq!(stdout, "");
        assert_eq!(stderr, "Error: empty output for zpool status\n");
        assert!(!status.success());
    }

    Ok(())
}

#[test]
#[ignore = "takes 15+ seconds"]
fn child_sleep_forever() -> anyhow::Result<()> {
    const LISTEN_ADDRESS: &str = crate::common::LISTEN_ADDRESS_CHILD_SILENT_2;

    let mut child = BinCommand::new()
        .arg(LISTEN_ADDRESS)
        .fake_zpool_mode(FakeZpoolMode::SleepForever)
        .spawn()?;

    let start_time = Instant::now();
    while start_time.elapsed() < Duration::from_secs(30) {
        if child.is_finished().unwrap() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(child.is_finished().unwrap(), "child should timeout on time");

    let output = child.kill_await_output()?;

    {
        let BinOutput {
            status,
            stdout,
            stderr,
        } = output;

        assert_eq!(stdout, "");
        assert!(
            stderr
                .lines()
                .any(|line| line.trim().starts_with("command timed out")),
            "stderr {stderr:?}"
        );
        assert!(!status.success());
    }

    Ok(())
}
