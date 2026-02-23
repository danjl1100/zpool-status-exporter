use crate::common::bin_cmd::{BinCommand, BinOutput, FakeZpoolMode};

#[test]
fn oneshot() -> anyhow::Result<()> {
    const ONESHOT: &str = "--oneshot-test-print";
    const EXPECTED_OUTPUT: &str = "# no pools reported\n# HELP zpool_lookup total duration of the lookup in seconds\n# TYPE zpool_lookup gauge\nzpool_lookup";

    let (output, ()) = BinCommand::new()
        .arg(ONESHOT)
        .fake_zpool_mode(FakeZpoolMode::NoPools)
        .spawn_cleanup_with(|| {})?;

    {
        let BinOutput {
            status,
            stdout,
            stderr,
        } = output;

        assert!(
            stdout.starts_with(EXPECTED_OUTPUT),
            "stdout did not start with expected, got: {stdout:?}"
        );
        assert_eq!(stderr, "");
        assert!(status.success());
    }

    Ok(())
}
