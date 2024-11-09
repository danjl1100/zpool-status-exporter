use crate::common::bin_cmd::{BinCommand, BinOutput, FakeZpoolMode};

#[test]
fn exitcode_caught_1() -> anyhow::Result<()> {
    const LISTEN_ADDRESS: &str = crate::common::LISTEN_ADDRESS_CHILD_EXITCODE_1;

    let (output, ()) = BinCommand::new()
        .arg(LISTEN_ADDRESS)
        .fake_zpool_mode(FakeZpoolMode::ExitCode1)
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
            0: failed to execute zpool command
            1: command "zpool" (args ["status", "-p"]) failed with exit code exit status: 1,  stdout: "exit1 stdout contents\n", stderr: "exit1 stderr contents\n"
        "###);
        assert!(!status.success());
    }

    Ok(())
}

#[test]
fn exitcode_caught_2() -> anyhow::Result<()> {
    const LISTEN_ADDRESS: &str = crate::common::LISTEN_ADDRESS_CHILD_EXITCODE_2;

    let (output, ()) = BinCommand::new()
        .arg(LISTEN_ADDRESS)
        .fake_zpool_mode(FakeZpoolMode::ExitCode2)
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
            0: failed to execute zpool command
            1: command "zpool" (args ["status", "-p"]) failed with exit code exit status: 2,  stdout: "exit2 stdout contents\n", stderr: "exit2 stderr contents\n"
        "###);
        assert!(!status.success());
    }

    Ok(())
}
