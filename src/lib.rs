//! Prometheus-style exporter for `zpool status` numeric metrics
//!
//! The most notable output is the duration since the last scrub (if displayed)

// teach me
#![deny(clippy::pedantic)]
// // no unsafe
// #![forbid(unsafe_code)]
// sane unsafe
#![forbid(unsafe_op_in_unsafe_fn)]
// no unwrap
#![deny(clippy::unwrap_used)]
// no panic
#![deny(clippy::panic)]
// docs!
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use anyhow::Context as _;
use std::time::{Duration, Instant};
use time::{util::local_offset, UtcOffset};

pub mod fmt;
pub mod zfs;

/// Command-line arguments for the server
#[derive(clap::Parser)]
pub struct Args {
    /// Bind address for the server
    #[clap(env)]
    pub listen_address: std::net::SocketAddr,
}

/// Signal to cleanly terminate after finishing the current request (if any)
pub struct Shutdown;

/// System local-time context for calculating durations
#[must_use]
pub struct TimeContext {
    local_offset: UtcOffset,
}
impl TimeContext {
    /// Recommend to call this function in main, before all other actions
    /// (with no decorators on main, no async executors, etc.)
    ///
    /// # Safety
    ///
    /// Preconditions:
    ///  - There shall be no other threads in the process
    ///
    #[allow(clippy::missing_panics_doc)]
    pub unsafe fn new_unchecked() -> Self {
        let local_offset = {
            // SAFETY: caller has guaranteed no other threads exist in the process
            unsafe { local_offset::set_soundness(local_offset::Soundness::Unsound) };

            let local_offset = UtcOffset::current_local_offset();

            // SAFETY: called with `Soundness::Sound`
            unsafe { local_offset::set_soundness(local_offset::Soundness::Sound) };

            local_offset.expect("soundness temporarily disabled, to skip thread checks")
        };

        Self { local_offset }
    }

    /// Constructs a context for UTC only (not actually synchronized to the local time offset)
    pub fn new_assume_local_is_utc() -> Self {
        let local_offset = UtcOffset::UTC;
        Self { local_offset }
    }

    /// Spawn an HTTP server on the address specified by args
    ///
    /// # Errors
    ///
    /// Returns an error for any of the following:
    /// - binding the server fails
    /// - fail-fast metrics creation fails
    /// - shutdown receive fails (only if a `Receiver` was provided)
    ///
    pub fn serve(
        &self,
        args: &Args,
        mut shutdown_rx: Option<std::sync::mpsc::Receiver<Shutdown>>,
    ) -> anyhow::Result<()> {
        const RECV_TIMEOUT: Duration = Duration::from_millis(100);
        const RECV_SLEEP: Duration = Duration::from_millis(10);

        let Args { listen_address } = args;
        let server = tiny_http::Server::http(listen_address).map_err(|e| anyhow::anyhow!(e))?;

        // ensure fail-fast
        {
            self.timestamp_now().get_metrics_str()?;
        }

        println!("Listening at http://{listen_address:?}");

        while Self::check_shutdown(shutdown_rx.as_mut())?.is_none() {
            if let Some(request) = server.recv_timeout(RECV_TIMEOUT)? {
                self.timestamp_now().handle_request(request);
            } else {
                std::thread::sleep(RECV_SLEEP);
            }
        }
        Ok(())
    }
    fn check_shutdown(
        shutdown_rx: Option<&mut std::sync::mpsc::Receiver<Shutdown>>,
    ) -> anyhow::Result<Option<Shutdown>> {
        shutdown_rx
            .map(|rx| rx.try_recv())
            .transpose()
            .or_else(|err| {
                use std::sync::mpsc::TryRecvError as E;
                match err {
                    E::Disconnected => Err(anyhow::anyhow!("termination channel receive failure")),
                    E::Empty => {
                        // no shutdown signaled, yet
                        Ok(None)
                    }
                }
            })
    }
    /// Creates a new timestamp instance from the current date/time
    pub fn timestamp_now(&self) -> Timestamp<'_> {
        let datetime = time::OffsetDateTime::now_utc();
        let compute_time_start = Instant::now();
        self.timestamp_at(datetime, Some(compute_time_start))
    }
    /// Creates a new timestamp instance from the specified date/time
    pub fn timestamp_at(
        &self,
        datetime: time::OffsetDateTime,
        compute_time_start: Option<Instant>,
    ) -> Timestamp<'_> {
        Timestamp {
            time_context: self,
            datetime,
            compute_time_start,
        }
    }
}
/// Start time for parsing timestamps and formatting time-based metrics
#[must_use]
pub struct Timestamp<'a> {
    time_context: &'a TimeContext,
    datetime: time::OffsetDateTime,
    /// If present, start time for timing the computation
    compute_time_start: Option<Instant>,
}
impl<'a> Timestamp<'a> {
    fn handle_request(self, request: tiny_http::Request) {
        const ENDPOINT_METRICS: &str = "/metrics";
        const HTML_NOT_FOUND: u32 = 404;

        let result = {
            let url = request.url();
            if url == ENDPOINT_METRICS {
                let response = self.get_metrics_response();
                request.respond(response).context("metrics response")
            } else {
                let response = tiny_http::Response::empty(HTML_NOT_FOUND);
                request.respond(response).context("not-found response")
            }
        };
        if let Err(err) = result {
            eprintln!("failed to send response: {err:#}");
        }
    }
    // Infallible, returns commented error response on failure
    fn get_metrics_response(&self) -> tiny_http::Response<impl std::io::Read> {
        let response_str = self
            .get_metrics_str()
            .unwrap_or_else(|err| format!("# ERROR:\n# {err:#}"));
        tiny_http::Response::from_string(response_str)
    }

    fn get_metrics_str(&self) -> anyhow::Result<String> {
        let zpool_output = exec::zpool_status()?;
        self.get_metrics_for_output(&zpool_output)
    }

    /// Parses the `zpool_output` string and returns a formatted Prometheus-style metrics document
    ///
    /// # Errors
    /// Returns errors when [`TimeContext::parse_zfs_metrics`] fails
    pub fn get_metrics_for_output(&self, zpool_output: &str) -> anyhow::Result<String> {
        let zpool_metrics = self.time_context.parse_zfs_metrics(zpool_output)?;

        Ok(fmt::format_metrics(
            zpool_metrics,
            self.datetime,
            self.compute_time_start,
        ))
    }
}

pub mod exec {
    //! I/O portion of executing status commands

    use anyhow::Context;
    use std::process::Command;

    /// Returns the output of the `zpool status` command
    ///
    /// # Errors
    /// Returns an error if the command execution fails, or the output is non-utf8
    pub fn zpool_status() -> anyhow::Result<String> {
        const ARGS: &[&str] = &["status"];

        run_command("/sbin/zpool", ARGS)
            .or_else(|_| run_command("zpool", ARGS))
            .context("running \"zpool status\" command")
    }

    fn run_command(program: &str, args: &[&str]) -> anyhow::Result<String> {
        let command_output = Command::new(program)
            .args(args)
            .output()
            .with_context(|| format!("command {program:?} args {args:?}"))?;
        String::from_utf8(command_output.stdout).context("non-utf8 output")
    }
}
