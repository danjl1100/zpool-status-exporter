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

        insta::assert_snapshot!(stdout, @"");
        insta::assert_snapshot!(stderr, @r###"
        Error: failed to create metrics

        Caused by:
            empty output from zpool command
        "###);
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
        if child.is_finished()? {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(child.is_finished()?, "child should timeout on time");

    let output = child.kill_await_output()?;

    {
        let BinOutput {
            status,
            stdout,
            stderr,
        } = output;

        insta::assert_snapshot!(stdout, @"");
        insta::assert_snapshot!(stderr, @r###"
        Error: failed to create metrics

        Caused by:
            0: failed to execute zpool command
            1: command "zpool" (args ["status", "-p"]) should complete within the timeout
        "###);
        assert!(!status.success());
    }

    Ok(())
}
