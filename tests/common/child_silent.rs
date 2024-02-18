use crate::bin_cmd::{BinCommand, BinOutput, FakeZpoolMode};

#[test]
fn child_silent() -> anyhow::Result<()> {
    const LISTEN_ADDRESS: &str = crate::common::LISTEN_ADDRESS_CHILD_SILENT;

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
