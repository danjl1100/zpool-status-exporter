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

use std::time::Instant;
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

    /// Spawn an HTTP server on the address specified by args
    ///
    /// # Errors
    ///
    /// Returns an error if binding the server fails, or the fail-fast metrics creation fails
    pub fn serve(&self, args: &Args) -> anyhow::Result<()> {
        let Args { listen_address } = args;
        let server = tiny_http::Server::http(listen_address).map_err(|e| anyhow::anyhow!(e))?;

        // ensure fail-fast
        {
            let fake_start = Instant::now();
            self.get_metrics_str(fake_start)?;
        }

        println!("Listening at {listen_address:?}");

        loop {
            let request = server.recv()?;
            let _ = self.handle_request(request);
        }
    }
    fn handle_request(&self, request: tiny_http::Request) -> anyhow::Result<()> {
        const ENDPOINT_METRICS: &str = "/metrics";
        const HTML_NOT_FOUND: u32 = 404;

        let start_time = Instant::now();

        let url = request.url();
        if url == ENDPOINT_METRICS {
            let response = self.get_metrics_response(start_time);
            Ok(request.respond(response)?)
        } else {
            let response = tiny_http::Response::empty(HTML_NOT_FOUND);
            Ok(request.respond(response)?)
        }
    }
    fn get_metrics_response(&self, start_time: Instant) -> tiny_http::Response<impl std::io::Read> {
        let response_str = self
            .get_metrics_str(start_time)
            .unwrap_or_else(|err| format!("# ERROR:\n# {err:#}"));
        tiny_http::Response::from_string(response_str)
    }

    fn get_metrics_str(&self, start_time: Instant) -> anyhow::Result<String> {
        let zpool_output = exec::zpool_status()?;
        let zpool_metrics = self.parse_zfs_metrics(&zpool_output)?;
        Ok(fmt::format_metrics(zpool_metrics, start_time))
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
        run_command("zpool", &["status"]).context("running \"zpool status\" command")
    }

    fn run_command(program: &str, args: &[&str]) -> anyhow::Result<String> {
        let command_output = Command::new(program).args(args).output()?;
        String::from_utf8(command_output.stdout).context("non-utf8 output")
    }
}
