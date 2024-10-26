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
    args: Vec<String>,
}
impl BinCommand {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn fake_zpool_mode(mut self, mode: FakeZpoolMode) -> Self {
        self.mode = Some(mode);
        self
    }
    pub fn arg(mut self, arg: &'static str) -> Self {
        self.args.push(arg.to_string());
        self
    }
    pub fn arg_dynamic(mut self, arg: String) -> Self {
        self.args.push(arg);
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

        if !self.args.is_empty() {
            command.args(self.args);
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
    pub fn spawn_cleanup_with<T>(self, f: impl FnOnce() -> T) -> anyhow::Result<(BinOutput, T)> {
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
            Pid::from_raw(self.subcommand.id().try_into()?),
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
