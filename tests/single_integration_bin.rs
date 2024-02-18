//! Single integration test binary
//!
//! NOTE: Since the crate is primarily a "binary crate", the integration tests (running the
//! executable) are more important than library unit tests.
//!
//! As a general rule, there should only be one integration test binary, since integration tests
//! are run sequentially by cargo.
//!
//! Add as many `#[test]`s as you want! (in submodules of this `single_integration_bin`)

mod common {
    const LISTEN_ADDRESS_END_TO_END: &str = "127.0.0.1:9583";
    const LISTEN_ADDRESS_CHILD_STDERR_1: &str = "127.0.0.1:9584";
    const LISTEN_ADDRESS_CHILD_STDERR_2: &str = "127.0.0.1:9585";
    const LISTEN_ADDRESS_CHILD_SILENT_1: &str = "127.0.0.1:9586";
    const LISTEN_ADDRESS_CHILD_SILENT_2: &str = "127.0.0.1:9587";

    type MiniReqResult = Result<minreq::Response, minreq::Error>;

    mod child_silent;
    mod child_stderr;
    mod end_to_end;

    mod sans_io_cases;
}
const HTTP_NOT_FOUND: i32 = 404;
const HTTP_OK: i32 = 200;

/// line-by-line comparison, to filter out timestamp-sensitive items
fn assert_matches_template(response: &str, expected: &str) {
    const IGNORE_MARKER: &str = "<IGNORE>";

    println!("response:\n{response}\n--------------------------------------------------");
    println!("expected:\n{expected}\n--------------------------------------------------");

    let mut response = response.lines();
    let mut expected = expected.lines();
    loop {
        let response = response.next();
        let expected = expected.next();
        let (response, expected) = match (response, expected) {
            (None, None) => {
                break;
            }
            (Some(response), None) => {
                panic!("extra response line: {response:?}");
            }
            (None, Some(expected)) => {
                panic!("missing expected line: {expected:?}");
            }
            (Some(response), Some(expected)) => (response, expected),
        };
        assert_equals_ignore(response, expected, IGNORE_MARKER);
    }
}

fn assert_equals_ignore(response: &str, expected: &str, ignore: &str) {
    if expected.ends_with(ignore) {
        let (expected, after_ignore) = expected
            .split_once(ignore)
            .expect("contains marker because it also ends with marker");
        // SANITY - verify <IGNORE> is at end of line (e.g. only once in the line)
        assert_eq!(
            after_ignore, "",
            "only allowed one {ignore} per line, at end of line"
        );

        if response.len() < expected.len() {
            panic!("response too short for expected pattern\n\texpected = {expected:?}\n\tresponse = {response:?}");
        }
        let (response_trimmed, response_remainder) = response.split_at(expected.len());

        // SANITY - verify ignored portion is numeric
        if response_remainder.parse::<f64>().is_err() {
            panic!("non-numeric ignored remainder {response_remainder:?} of line {response:?}");
        };
        eprintln!("ignoring remainder {response_remainder:?} of line {response:?}");

        assert_eq!(response_trimmed, expected, "response_metrics line trimmed");
    } else {
        assert_eq!(response, expected, "response_metrics line");
    }
}

mod bin_cmd {
    use nix::{
        sys::signal::{self, Signal},
        unistd::Pid,
    };
    use std::{
        path::Path,
        process::{Child, Command, ExitStatus, Output, Stdio},
        time::Duration,
    };

    #[derive(Clone, Copy)]
    pub enum FakeZpoolMode {
        NoPools,
        DevsMissing,
        Silent,
        SleepForever,
    }
    #[derive(Default)]
    pub struct BinCommand {
        mode: Option<FakeZpoolMode>,
        arg: Option<String>,
    }
    impl BinCommand {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn fake_zpool_mode(mut self, mode: FakeZpoolMode) -> Self {
            self.mode = Some(mode);
            self
        }
        pub fn arg(mut self, arg: &str) -> Self {
            self.arg = Some(arg.to_string());
            self
        }
        fn build(self) -> Command {
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

            if let Some(mode) = self.mode {
                let mode_str = match mode {
                    FakeZpoolMode::NoPools => "no-pools",
                    FakeZpoolMode::DevsMissing => "devs-missing",
                    FakeZpoolMode::Silent => "silent",
                    FakeZpoolMode::SleepForever => "sleep-forever",
                };
                command.env("FAKE_ZPOOL_MODE", mode_str);
            }

            if let Some(arg) = self.arg {
                command.arg(arg);
            }

            command
        }
        pub fn spawn(self) -> anyhow::Result<BinChild> {
            let subcommand = self
                .build()
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            // allow grace period for init
            std::thread::sleep(Duration::from_millis(100));

            Ok(BinChild { subcommand })
        }
        pub fn spawn_cleanup_with<T>(
            self,
            f: impl FnOnce() -> T,
        ) -> anyhow::Result<(BinOutput, T)> {
            let mut child = self.spawn()?;
            let fn_output = f();

            child.interrupt_wait()?;
            let output = child.kill_await_output()?;

            Ok((output, fn_output))
        }
    }

    pub struct BinChild {
        subcommand: Child,
    }
    impl BinChild {
        pub fn interrupt_wait(&mut self) -> anyhow::Result<()> {
            // SIGINT - request clean exit
            signal::kill(
                Pid::from_raw(self.subcommand.id().try_into().unwrap()),
                Signal::SIGINT,
            )?;

            // allow grace period for cleanup
            std::thread::sleep(Duration::from_millis(300));

            Ok(())
        }
        pub fn kill_await_output(mut self) -> anyhow::Result<BinOutput> {
            self.subcommand.kill()?;

            let output = self.subcommand.wait_with_output()?;
            let output = BinOutput::new(output)?;

            Ok(output)
        }
        pub fn is_finished(&mut self) -> anyhow::Result<bool> {
            let wait_result = self.subcommand.try_wait()?;
            Ok(wait_result.is_some())
        }
    }

    pub struct BinOutput {
        pub status: ExitStatus,
        pub stdout: String,
        pub stderr: String,
    }
    impl BinOutput {
        fn new(output: Output) -> anyhow::Result<Self> {
            let Output {
                status,
                stdout,
                stderr,
            } = output;
            let stdout = String::from_utf8(stdout)?;
            let stderr = String::from_utf8(stderr)?;
            Ok(Self {
                status,
                stdout,
                stderr,
            })
        }
    }
}
