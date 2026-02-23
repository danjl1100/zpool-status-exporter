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

pub use metrics::Error as MetricsError;
pub use server::Builder as ServerBuilder;
pub use server::Error as ServerError;
use std::time::Instant;
use tinytemplate::TinyTemplate;
pub use zfs::ParseError as ZfsParseError;

mod auth;
mod fmt;
mod zfs;

/// Command-line arguments for the server
#[must_use]
pub struct Args {
    /// Bind address for the server
    listen_address: std::net::SocketAddr,
    /// Filename containing allowed basic authentication tokens
    basic_auth_keys_file: Option<std::path::PathBuf>,
    /// Maximum number of bind retry attempts
    max_bind_retries: u32,
}
impl Args {
    /// Configure listenining with basic authentication
    pub fn listen_basic_auth(
        listen_address: std::net::SocketAddr,
        basic_auth_keys_file: Option<std::path::PathBuf>,
        max_bind_retries: u32,
    ) -> Self {
        Self {
            listen_address,
            basic_auth_keys_file,
            max_bind_retries,
        }
    }
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
    #[allow(missing_docs)]
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
    pub fn get_metrics_now(&self) -> Result<String, MetricsError> {
        self.timestamp_now().get_metrics_str()
    }
}

mod server {
    use crate::{
        AppContext, Args, MetricsError, Ready, Shutdown,
        auth::{self, AuthRules},
    };
    use std::{net::SocketAddr, time::Duration};

    /// Calculate delay for exponential backoff
    pub(crate) fn calculate_delay_seconds(attempt: u32) -> u64 {
        (1u64 << (attempt - 1)).min(16) // 1, 2, 4, 8, 16, 16, 16... seconds (capped at 16)
    }

    /// Start server with retry logic
    pub(crate) fn start_server_with_retry(
        listen_address: SocketAddr,
        max_retries: u32,
    ) -> Result<tiny_http::Server, Error> {
        let mut attempt = 1;
        let mut retries_remaining = max_retries;

        loop {
            // Attempt connection
            match tiny_http::Server::http(listen_address) {
                Ok(server) => {
                    if attempt > 1 {
                        println!("Successfully bound to {listen_address} on attempt {attempt}");
                    }
                    return Ok(server);
                }
                Err(e) => {
                    // Check retries remaining
                    if retries_remaining == 0 {
                        // Return error when exhausted
                        return Err(Error {
                            kind: ErrorKind::HttpServerBind {
                                io_error: e,
                                listen_address,
                            },
                        });
                    }

                    // Delay and continue if retries available
                    let delay_secs = calculate_delay_seconds(attempt);
                    eprintln!("Bind attempt {attempt} failed: {e}. Retrying in {delay_secs}s...");
                    std::thread::sleep(Duration::from_secs(delay_secs));

                    attempt += 1;
                    retries_remaining -= 1;
                }
            }
        }
    }

    /// Configuration for an HTTP server
    #[must_use]
    pub struct Builder<'a> {
        app_context: &'a AppContext,
        args: &'a Args,
        ready_tx: Option<std::sync::mpsc::Sender<Ready>>,
        shutdown_rx: Option<std::sync::mpsc::Receiver<Shutdown>>,
    }

    impl AppContext {
        /// Returns an HTTP server builder
        pub fn server_builder<'a>(&'a self, args: &'a Args) -> Builder<'a> {
            Builder {
                app_context: self,
                args,
                ready_tx: None,
                shutdown_rx: None,
            }
        }
    }
    impl Builder<'_> {
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

        /// Spawn a blocking HTTP server on the address specified by args
        ///
        /// # Errors
        ///
        /// Returns an error for any of the following:
        /// - binding the server fails
        /// - fail-fast metrics creation fails
        /// - shutdown receive fails (only if a `Receiver` was provided)
        /// - loading the auth key file fails
        ///
        pub fn serve(self) -> Result<(), Error> {
            let Self {
                app_context,
                args:
                    Args {
                        listen_address,
                        basic_auth_keys_file,
                        max_bind_retries,
                    },
                mut ready_tx,
                mut shutdown_rx,
            } = self;

            let make_error = |kind| Error { kind };

            let auth_rules = basic_auth_keys_file
                .as_ref()
                .map(AuthRules::from_file)
                .transpose()
                .map_err(ErrorKind::AuthFile)
                .map_err(make_error)?;

            let server = start_server_with_retry(*listen_address, *max_bind_retries)?;

            // ensure fail-fast
            {
                app_context
                    .get_metrics_now()
                    .map_err(ErrorKind::Metrics)
                    .map_err(make_error)?;
            }

            println!("Listening at http://{listen_address:?}");
            if let Some(auth_rules) = &auth_rules {
                auth_rules.print_start_message();
            }

            if let Some(ready_tx) = ready_tx.take() {
                // ignore "ready" receive errors
                let _ = ready_tx.send(Ready);
            }

            while Self::check_shutdown(shutdown_rx.as_mut()).is_none() {
                let response_result =
                    Self::serve_next_peer(&server, app_context, auth_rules.as_ref());
                if let Err(error) = response_result {
                    eprintln!("failed to send response: {error}");
                    // TODO log error to console, cannot shutdown server for peer errors
                }
            }
            Ok(())
        }
        fn check_shutdown(
            shutdown_rx: Option<&mut std::sync::mpsc::Receiver<Shutdown>>,
        ) -> Option<Shutdown> {
            shutdown_rx
                .map(|rx| rx.try_recv())
                .transpose()
                .unwrap_or_else(|err| {
                    use std::sync::mpsc::TryRecvError as E;
                    match err {
                        E::Disconnected => {
                            eprintln!("termination channel receive failure");
                            Some(Shutdown)
                            // TODO log an error to the console
                        }
                        E::Empty => {
                            // no shutdown signaled, yet
                            None
                        }
                    }
                })
        }
    }

    /// Error establishing the server
    #[derive(Debug)]
    pub struct Error {
        kind: ErrorKind,
    }
    /// For creating the server (report error to caller)
    #[derive(Debug)]
    enum ErrorKind {
        AuthFile(auth::FileError),
        Metrics(MetricsError),
        HttpServerBind {
            io_error: Box<dyn std::error::Error + Send + Sync>,
            listen_address: SocketAddr,
        },
    }
    impl std::error::Error for Error {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match &self.kind {
                ErrorKind::AuthFile(error) => Some(error),
                ErrorKind::Metrics(error) => Some(error),
                ErrorKind::HttpServerBind { io_error, .. } => Some(&**io_error),
            }
        }
    }
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self { kind } = self;
            match kind {
                ErrorKind::AuthFile(_error) => write!(f, "invalid auth file"),
                ErrorKind::Metrics(_error) => write!(f, "failed to create metrics"),
                ErrorKind::HttpServerBind {
                    io_error: _,
                    listen_address,
                } => write!(f, "failed to bind HTTP server to {listen_address}"),
            }
        }
    }
}

mod respond {
    use crate::{
        AppContext, MetricsError, ServerBuilder, Timestamp,
        auth::{self, AuthResult, AuthRules, DebugUserStringRef},
    };
    use std::time::Duration;

    pub(super) const HTTP_BAD_REQUEST: (u32, &str) = (400, "Bad Request");
    pub(super) const HTTP_UNAUTHORIZED: (u32, &str) = (401, "Unauthorized");
    pub(super) const HTTP_FORBIDDEN: (u32, &str) = (403, "Forbidden");
    pub(super) const HTTP_NOT_FOUND: (u32, &str) = (404, "Not Found");
    pub(super) fn respond_code(
        request: tiny_http::Request,
        code_label: (u32, &'static str),
        header: Option<tiny_http::Header>,
    ) -> Result<(), Error> {
        let (code, label) = code_label;
        let mut response = tiny_http::Response::from_string(label).with_status_code(code);

        if let Some(header) = header {
            response = response.with_header(header);
        }

        request
            .respond(response)
            .map_err(Endpoint::Code(code_label).error_fn())
    }

    impl ServerBuilder<'_> {
        pub(super) fn serve_next_peer(
            server: &tiny_http::Server,
            app_context: &AppContext,
            auth_rules: Option<&AuthRules>,
        ) -> Result<(), Error> {
            const RECV_TIMEOUT: Duration = Duration::from_millis(100);
            const RECV_SLEEP: Duration = Duration::from_millis(10);

            if let Some(request) = server
                .recv_timeout(RECV_TIMEOUT)
                .map_err(|io_error| Error {
                    io_error,
                    kind: ErrorKind::PeerReceive,
                })?
            {
                let auth_result = auth_rules.map_or(Ok(AuthResult::NoneConfigured), |auth_rules| {
                    auth_rules.query(&request)
                });
                match auth_result {
                    Ok(auth_result) => app_context
                        .timestamp_now()
                        .handle_request(request, auth_result),
                    Err(err) => {
                        println!("{err}");
                        respond_code(request, HTTP_BAD_REQUEST, None)
                    }
                }
            } else {
                std::thread::sleep(RECV_SLEEP);
                Ok(())
            }
        }
    }

    impl Timestamp<'_> {
        pub(crate) fn handle_request(
            self,
            request: tiny_http::Request,
            auth: AuthResult,
        ) -> Result<(), Error> {
            const ENDPOINT_METRICS: &str = "/metrics";
            const ENDPOINT_ROOT: &str = "/";

            let url = request.url();
            if url == ENDPOINT_ROOT {
                let response = self.get_public_root_response();
                request.respond(response).map_err(Endpoint::Root.error_fn())
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
                            let (response, metrics_result) = self.get_metrics_response();
                            if let Err(err) = metrics_result {
                                eprintln!("failed to get metrics: {err}");
                                // TODO log to console
                            }
                            request
                                .respond(response)
                                .map_err(Endpoint::Metrics.error_fn())
                        } else {
                            respond_code(request, HTTP_NOT_FOUND, None)
                        }
                    }
                }
            }
        }
        fn get_public_root_response(&self) -> tiny_http::Response<impl std::io::Read> {
            let root_html = self.app_context.render_root_html();

            tiny_http::Response::from_string(root_html).with_header(
                tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..])
                    .expect("valid hard-coded content type header"),
            )
        }
        // Infallible, returns commented error response on failure
        pub(crate) fn get_metrics_response(
            &self,
        ) -> (
            tiny_http::Response<impl std::io::Read>,
            Result<(), MetricsError>,
        ) {
            use std::fmt::Write as _;

            let (response_str, metrics_result) = match self.get_metrics_str() {
                Ok(metrics_str) => (metrics_str, Ok(())),
                Err(err) => {
                    let mut response_str = "# ERROR:".to_owned();
                    for line in err.to_string().lines() {
                        write!(&mut response_str, "\n# {line}").expect("string write infallible");
                    }
                    (response_str, Err(err))
                }
            };
            let response = tiny_http::Response::from_string(response_str);
            (response, metrics_result)
        }
    }

    #[derive(Debug)]
    pub struct Error {
        io_error: std::io::Error,
        kind: ErrorKind,
    }
    impl Endpoint {
        fn error_fn(self) -> impl Fn(std::io::Error) -> Error {
            move |io_error| Error {
                io_error,
                kind: ErrorKind::Endpoint(self),
            }
        }
    }
    #[derive(Debug)]
    enum ErrorKind {
        PeerReceive,
        Endpoint(Endpoint),
    }
    #[derive(Clone, Copy, Debug)]
    enum Endpoint {
        Code((u32, &'static str)),
        Root,
        Metrics,
    }
    impl std::error::Error for Error {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.io_error)
        }
    }
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self { io_error: _, kind } = self;
            match kind {
                ErrorKind::PeerReceive => write!(f, "failed to receive from peer"),
                ErrorKind::Endpoint(endpoint) => {
                    write!(f, "failed to respond to peer with ")?;
                    match endpoint {
                        Endpoint::Code((code, label)) => write!(f, "code {code} {label}"),
                        Endpoint::Root => write!(f, "root endpoint"),
                        Endpoint::Metrics => write!(f, "metrics endpoint"),
                    }
                }
            }
        }
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

/// Start time for parsing timestamps and formatting time-based metrics
#[must_use]
pub struct Timestamp<'a> {
    app_context: &'a AppContext,
    datetime: jiff::Zoned,
    /// If present, start time for timing the computation
    compute_time_start: Option<Instant>,
}
mod metrics {
    use crate::{Timestamp, ZfsParseError, exec, fmt};

    impl Timestamp<'_> {
        pub(crate) fn get_metrics_str(&self) -> Result<String, Error> {
            let make_error = |kind| Error { kind };

            let zpool_output = exec::zpool_status()
                .map_err(ErrorKind::Exec)
                .map_err(make_error)?;

            if zpool_output.is_empty() {
                Err(make_error(ErrorKind::EmptyOutput))
            } else {
                self.get_metrics_for_output(&zpool_output)
                    .map_err(ErrorKind::ZfsParse)
                    .map_err(make_error)
            }
        }

        /// Parses the `zpool_output` string and returns a formatted Prometheus-style metrics document
        ///
        /// # Errors
        /// Returns errors when parsing ZFS metrics fails
        pub fn get_metrics_for_output(&self, zpool_output: &str) -> Result<String, ZfsParseError> {
            let zpool_metrics = self.app_context.parse_zfs_metrics(zpool_output)?;

            Ok(fmt::format_metrics(
                zpool_metrics,
                &self.datetime,
                self.compute_time_start,
            ))
        }
    }

    /// Error obtaining zpool status metrics from the system
    #[derive(Debug)]
    pub struct Error {
        kind: ErrorKind,
    }
    #[derive(Debug)]
    enum ErrorKind {
        Exec(exec::Error),
        EmptyOutput,
        ZfsParse(ZfsParseError),
    }
    impl std::error::Error for Error {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match &self.kind {
                ErrorKind::Exec(error) => Some(error),
                ErrorKind::EmptyOutput => None,
                ErrorKind::ZfsParse(error) => Some(error),
            }
        }
    }
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self { kind } = self;
            let description = match kind {
                ErrorKind::Exec(_error) => "failed to execute",
                ErrorKind::EmptyOutput => "empty output from",
                ErrorKind::ZfsParse(_error) => "failed to parse output from",
            };
            write!(f, "{description} zpool command")
        }
    }
}

mod exec {
    //! I/O portion of executing status commands

    use std::{
        process::{Command, Output, Stdio},
        time::{Duration, Instant},
    };

    /// Returns the output of the `zpool status` command
    ///
    /// # Errors
    /// Returns an error if the command execution fails, or the output is non-utf8
    pub fn zpool_status() -> Result<String, Error> {
        // NOTE: "-p" for parsable (exact) values in the device table
        const ARGS: &[&str] = &["status", "-p"];

        run_command("/sbin/zpool", ARGS).or_else(|err| {
            if err.is_spawn_error() {
                run_command("zpool", ARGS)
            } else {
                Err(err)
            }
        })
    }

    fn run_command(program: &'static str, args: &'static [&'static str]) -> Result<String, Error> {
        const TIMEOUT: Duration = Duration::from_secs(15);

        let make_error = |kind| Error {
            command: program,
            args,
            kind,
        };

        let mut subcommand = Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(ErrorKind::ChildSpawn)
            .map_err(make_error)?;

        let start_time = Instant::now();

        let mut wait = 1;
        loop {
            if start_time.elapsed() >= TIMEOUT {
                subcommand
                    .kill()
                    .map_err(ErrorKind::ChildTerminate)
                    .map_err(make_error)?;
                return Err(make_error(ErrorKind::Timeout));
            }
            if subcommand
                .try_wait()
                .map_err(ErrorKind::ChildStatus)
                .map_err(make_error)?
                .is_some()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(wait));
            wait *= 2;
        }

        let Output {
            status,
            stdout,
            stderr,
        } = subcommand
            .wait_with_output()
            .map_err(ErrorKind::ChildOutput)
            .map_err(make_error)?;

        if !status.success() {
            return Err(make_error(ErrorKind::ChildFailed {
                status,
                stdout: String::from_utf8_lossy(&stdout).to_string(),
                stderr: String::from_utf8_lossy(&stderr).to_string(),
            }));
        }

        let output = if stdout.is_empty() { stderr } else { stdout };

        String::from_utf8(output)
            .map_err(ErrorKind::NonUtf8Output)
            .map_err(make_error)
    }

    #[derive(Debug)]
    pub struct Error {
        command: &'static str,
        args: &'static [&'static str],
        kind: ErrorKind,
    }
    #[derive(Debug)]
    enum ErrorKind {
        ChildSpawn(std::io::Error),
        ChildStatus(std::io::Error),
        ChildOutput(std::io::Error),
        ChildTerminate(std::io::Error),
        Timeout,
        NonUtf8Output(std::string::FromUtf8Error),
        ChildFailed {
            status: std::process::ExitStatus,
            stdout: String,
            stderr: String,
        },
    }
    impl std::error::Error for Error {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match &self.kind {
                ErrorKind::ChildSpawn(err)
                | ErrorKind::ChildStatus(err)
                | ErrorKind::ChildOutput(err)
                | ErrorKind::ChildTerminate(err) => Some(err),
                ErrorKind::Timeout | ErrorKind::ChildFailed { .. } => None,
                ErrorKind::NonUtf8Output(err) => Some(err),
            }
        }
    }
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self {
                command,
                args,
                kind,
            } = self;
            let kind_str = match kind {
                ErrorKind::ChildSpawn(_) => "should spawn a process",
                ErrorKind::ChildStatus(_) => "should have a retrievable exit status",
                ErrorKind::ChildOutput(_) => "should have a retrievable output",
                ErrorKind::ChildTerminate(_) => "should terminate successfully",
                ErrorKind::Timeout => "should complete within the timeout",
                ErrorKind::NonUtf8Output(_) => "should output valid UTF-8",
                ErrorKind::ChildFailed {
                    status,
                    stdout,
                    stderr,
                } => &format!(
                    "failed with exit code {status},  stdout: {stdout:?}, stderr: {stderr:?}"
                ),
            };
            write!(f, "command {command:?} (args {args:?}) {kind_str}")
        }
    }
    impl Error {
        fn is_spawn_error(&self) -> bool {
            matches!(self.kind, ErrorKind::ChildSpawn(_))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::server::{calculate_delay_seconds, start_server_with_retry};
    use std::net::{SocketAddr, TcpListener};

    #[test]
    fn test_calculate_delay_seconds() {
        // Test exponential backoff: 1, 2, 4, 8, 16, 16, 16...
        assert_eq!(calculate_delay_seconds(1), 1);
        assert_eq!(calculate_delay_seconds(2), 2);
        assert_eq!(calculate_delay_seconds(3), 4);
        assert_eq!(calculate_delay_seconds(4), 8);
        assert_eq!(calculate_delay_seconds(5), 16);
        assert_eq!(calculate_delay_seconds(6), 16); // capped at 16
        assert_eq!(calculate_delay_seconds(10), 16); // capped at 16
    }

    #[test]
    fn test_no_retries_single_attempt() {
        // Bind to an occupied port to force failure
        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind test socket");
        let local_addr = listener.local_addr().expect("Failed to get local address");

        // Test that 0 retries = exactly 1 attempt
        let result = start_server_with_retry(local_addr, 0);
        assert!(result.is_err());

        if let Err(error) = result {
            let error_message = error.to_string();
            assert!(error_message.contains("failed to bind HTTP server"));
        }
    }

    #[test]
    fn test_successful_bind_first_attempt() {
        // Use port 0 to let the OS assign an available port
        let addr: SocketAddr = "127.0.0.1:0".parse().expect("Valid socket address");

        let result = start_server_with_retry(addr, 5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_successful_bind_with_retries_available() {
        // Test that having retries available doesn't affect successful first attempt
        let addr: SocketAddr = "127.0.0.1:0".parse().expect("Valid socket address");

        let result = start_server_with_retry(addr, 10);
        assert!(result.is_ok());
    }
}
