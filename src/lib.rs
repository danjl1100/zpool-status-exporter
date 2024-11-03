//! Prometheus-style exporter for `zpool status` numeric metrics
//!
//! The most notable output is the duration since the last scrub (if displayed)
//!
//! ---
//!
//! This crate is accurately described as an attempt at "the more brittle text parsing required".
//!
//! Inspired by a comment on an issue in
//! [github.com:pdf/zfs_exporter](https://github.com/pdf/zfs_exporter/issues/20#issuecomment-1047249253).
//!

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

use crate::auth::{AuthResult, AuthRules, DebugUserStringRef};
use anyhow::Context as _;
use std::time::{Duration, Instant};
use tinytemplate::TinyTemplate;

pub mod auth;
pub mod fmt;
pub mod zfs;

/// Command-line arguments for the server
#[derive(clap::Parser)]
#[clap(version)]
pub struct Args {
    /// Bind address for the server
    #[clap(env)]
    pub listen_address: std::net::SocketAddr,
    /// Filename containing allowed basic authentication tokens
    #[clap(env)]
    #[arg(long)]
    pub basic_auth_keys_file: Option<std::path::PathBuf>,
}

/// Signal to cleanly terminate after finishing the current request (if any)
pub struct Shutdown;

/// Signal that the server is ready to receive requests
pub struct Ready;

const TEMPLATE_ROOT_NAME: &str = "root";

/// System local-time context for calculating durations
#[must_use]
pub struct AppContext {
    timezone: jiff::tz::TimeZone,
    templates: TinyTemplate<'static>,
    template_context: TemplateContext,
}

#[derive(serde::Serialize)]
struct TemplateContext {
    name_suffix: String,
}

impl Default for AppContext {
    fn default() -> Self {
        Self::new()
    }
}
impl AppContext {
    /// Recommend to call this function in main, before all other actions
    /// (with no decorators on main, no async executors, etc.)
    ///
    /// # Safety
    ///
    /// Preconditions:
    ///  - There shall be no other threads in the process
    ///
    #[allow(clippy::missing_panics_doc)]
    pub fn new() -> Self {
        Self::new_with_timezone(jiff::tz::TimeZone::system())
    }

    /// Constructs a context for UTC only (not actually synchronized to the local time offset)
    pub fn new_assume_local_is_utc() -> Self {
        Self::new_with_timezone(jiff::tz::TimeZone::UTC)
    }

    fn new_with_timezone(timezone: jiff::tz::TimeZone) -> Self {
        const ROOT_HTML: &str = include_str!("root.html");

        let mut templates = TinyTemplate::new();
        templates
            .add_template(TEMPLATE_ROOT_NAME, ROOT_HTML)
            .expect("root.html should be a valid template");

        let template_context = TemplateContext {
            name_suffix: String::new(),
        };

        Self {
            timezone,
            templates,
            template_context,
        }
    }

    /// Sets the app version string for the root page
    pub fn set_app_version(&mut self, app_version: Option<&str>) {
        if let Some(app_version) = app_version {
            self.template_context.name_suffix = format!(" v{app_version}");
        } else {
            self.template_context.name_suffix.clear();
        }
    }

    fn render_root_html(&self) -> String {
        self.templates
            .render(TEMPLATE_ROOT_NAME, &self.template_context)
            .expect("root.html template should render as valid")
    }

    /// Returns the current metrics as a string (no server)
    ///
    /// # Errors
    /// Returns an error if the command execution fails, the output is non-utf8, or parsing fails
    pub fn get_metrics_now(&self) -> anyhow::Result<String> {
        self.timestamp_now().get_metrics_str()
    }

    /// Returns an HTTP server builder
    pub fn server_builder<'a>(&'a self, args: &'a Args) -> ServerBuilder<'a> {
        ServerBuilder {
            app_context: self,
            args,
            ready_tx: None,
            shutdown_rx: None,
        }
    }
}

/// Configuration for an HTTP server
#[must_use]
pub struct ServerBuilder<'a> {
    app_context: &'a AppContext,
    args: &'a Args,
    ready_tx: Option<std::sync::mpsc::Sender<Ready>>,
    shutdown_rx: Option<std::sync::mpsc::Receiver<Shutdown>>,
}

impl ServerBuilder<'_> {
    /// Sets the sender to be notified when the server is [`Ready`]
    pub fn set_ready_sender(mut self, ready_tx: std::sync::mpsc::Sender<Ready>) -> Self {
        self.ready_tx = Some(ready_tx);
        self
    }

    /// Sets the receiver for the server [`Shutdown`] signal
    pub fn set_shutdown_receiver(
        mut self,
        shutdown_rx: std::sync::mpsc::Receiver<Shutdown>,
    ) -> Self {
        self.shutdown_rx = Some(shutdown_rx);
        self
    }

    /// Spawn an HTTP server on the address specified by args
    ///
    /// # Errors
    ///
    /// Returns an error for any of the following:
    /// - binding the server fails
    /// - fail-fast metrics creation fails
    /// - shutdown receive fails (only if a `Receiver` was provided)
    /// - loading the auth key file fails
    ///
    pub fn serve(self) -> anyhow::Result<()> {
        const RECV_TIMEOUT: Duration = Duration::from_millis(100);
        const RECV_SLEEP: Duration = Duration::from_millis(10);

        let Self {
            app_context,
            args:
                Args {
                    listen_address,
                    basic_auth_keys_file,
                },
            mut ready_tx,
            mut shutdown_rx,
        } = self;

        let auth_rules = basic_auth_keys_file
            .as_ref()
            .map(|file| {
                AuthRules::from_file(file)
                    .with_context(|| format!("reading basic_auth_keys_file {:?}", file.display()))
            })
            .transpose()?;

        let server = tiny_http::Server::http(listen_address).map_err(|e| anyhow::anyhow!(e))?;

        // ensure fail-fast
        {
            app_context.get_metrics_now()?;
        }

        println!("Listening at http://{listen_address:?}");
        if let Some(auth_rules) = &auth_rules {
            auth_rules.print_start_message();
        }

        if let Some(ready_tx) = ready_tx.take() {
            // ignore "ready" receive errors
            let _ = ready_tx.send(Ready);
        }

        while Self::check_shutdown(shutdown_rx.as_mut())?.is_none() {
            if let Some(request) = server.recv_timeout(RECV_TIMEOUT)? {
                let auth_result = auth_rules
                    .as_ref()
                    .map_or(Ok(AuthResult::NoneConfigured), |auth_rules| {
                        auth_rules.query(&request)
                    });
                match auth_result {
                    Ok(auth_result) => app_context
                        .timestamp_now()
                        .handle_request(request, auth_result),
                    Err(auth::InvalidHeaderError(err)) => {
                        dbg!(err);
                        respond_code(request, HTTP_BAD_REQUEST, None)?;
                    }
                }
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
}

impl AppContext {
    /// Creates a new timestamp instance from the current date/time
    pub fn timestamp_now(&self) -> Timestamp<'_> {
        let datetime = jiff::Zoned::now();
        let compute_time_start = Instant::now();
        self.timestamp_at(datetime, Some(compute_time_start))
    }
    /// Creates a new timestamp instance from the specified UNIX UTC timestamp, or `None` if the
    /// timestamp is out of bounds
    #[must_use]
    pub fn timestamp_at_unix_utc(
        &self,
        unix_utc_timestamp: i64,
        compute_time_start: Option<Instant>,
    ) -> Option<Timestamp<'_>> {
        let datetime = jiff::Timestamp::from_second(unix_utc_timestamp)
            .ok()?
            .to_zoned(jiff::tz::TimeZone::UTC);
        Some(self.timestamp_at(datetime, compute_time_start))
    }
    fn timestamp_at(
        &self,
        datetime: jiff::Zoned,
        compute_time_start: Option<Instant>,
    ) -> Timestamp<'_> {
        Timestamp {
            app_context: self,
            datetime,
            compute_time_start,
        }
    }
}

const HTTP_BAD_REQUEST: (u32, &str) = (400, "Bad Request");
const HTTP_UNAUTHORIZED: (u32, &str) = (401, "Unauthorized");
const HTTP_FORBIDDEN: (u32, &str) = (403, "Forbidden");
const HTTP_NOT_FOUND: (u32, &str) = (404, "Not Found");
fn respond_code(
    request: tiny_http::Request,
    (code, label): (u32, &str),
    header: Option<tiny_http::Header>,
) -> anyhow::Result<()> {
    let mut response = tiny_http::Response::from_string(label).with_status_code(code);

    if let Some(header) = header {
        response = response.with_header(header);
    }

    request
        .respond(response)
        .with_context(|| format!("{code} response"))
}

/// Start time for parsing timestamps and formatting time-based metrics
#[must_use]
pub struct Timestamp<'a> {
    app_context: &'a AppContext,
    datetime: jiff::Zoned,
    /// If present, start time for timing the computation
    compute_time_start: Option<Instant>,
}
impl Timestamp<'_> {
    fn handle_request(self, request: tiny_http::Request, auth: AuthResult) {
        const ENDPOINT_METRICS: &str = "/metrics";
        const ENDPOINT_ROOT: &str = "/";

        let url = request.url();
        let result = if url == ENDPOINT_ROOT {
            let response = self.get_public_root_response();
            request.respond(response).context("root response")
        } else {
            match auth {
                AuthResult::MissingAuthHeader => respond_code(
                    request,
                    HTTP_UNAUTHORIZED,
                    Some(auth::get_header_www_authenticate()),
                ),
                AuthResult::Deny(who) => {
                    println!(
                        "denied access for {who} to url {url}",
                        url = DebugUserStringRef::from(url)
                    );
                    respond_code(request, HTTP_FORBIDDEN, None)
                }
                AuthResult::Accept | AuthResult::NoneConfigured => {
                    if url == ENDPOINT_METRICS {
                        let response = self.get_metrics_response();
                        request.respond(response).context("metrics response")
                    } else {
                        respond_code(request, HTTP_NOT_FOUND, None)
                    }
                }
            }
        };
        if let Err(err) = result {
            eprintln!("failed to send response: {err:#}");
        }
    }
    fn get_public_root_response(&self) -> tiny_http::Response<impl std::io::Read> {
        let root_html = self.app_context.render_root_html();

        tiny_http::Response::from_string(root_html).with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..])
                .expect("valid header"),
        )
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
    /// Returns errors when parsing ZFS metrics fails
    pub fn get_metrics_for_output(&self, zpool_output: &str) -> anyhow::Result<String> {
        let zpool_metrics = self.app_context.parse_zfs_metrics(zpool_output)?;

        Ok(fmt::format_metrics(
            zpool_metrics,
            &self.datetime,
            self.compute_time_start,
        ))
    }
}

mod exec {
    //! I/O portion of executing status commands

    use anyhow::Context;
    use std::{
        process::{Command, Output, Stdio},
        time::{Duration, Instant},
    };

    /// Returns the output of the `zpool status` command
    ///
    /// # Errors
    /// Returns an error if the command execution fails, or the output is non-utf8
    pub fn zpool_status() -> anyhow::Result<String> {
        const ARGS: &[&str] = &["status"];

        let output = run_command("/sbin/zpool", ARGS)
            .or_else(|_| run_command("zpool", ARGS))
            .context("running \"zpool status\" command")?;
        if output.is_empty() {
            anyhow::bail!("empty output for zpool status")
        }
        Ok(output)
    }

    fn run_command(program: &str, args: &[&str]) -> anyhow::Result<String> {
        const TIMEOUT: Duration = Duration::from_secs(15);

        let mut subcommand = Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("command {program:?} args {args:?}"))?;

        let start_time = Instant::now();

        let mut wait = 1;
        loop {
            if start_time.elapsed() >= TIMEOUT {
                subcommand.kill()?;
                anyhow::bail!("command timed out: {program:?} args {args:?}");
            }
            if subcommand.try_wait()?.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(wait));
            wait *= 2;
        }

        let Output {
            status: _,
            stdout,
            stderr,
        } = subcommand.wait_with_output()?;
        let output = if stdout.is_empty() { stderr } else { stdout };

        String::from_utf8(output).context("non-utf8 output")
    }
}
