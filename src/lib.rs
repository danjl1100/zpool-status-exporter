#![forbid(unsafe_op_in_unsafe_fn)]

use clap::Parser;
use std::time::Instant;
use time::{util::local_offset, UtcOffset};
use tiny_http::{Request, Response, Server};

#[derive(Parser)]
pub struct Args {
    #[clap(env)]
    pub listen_address: std::net::SocketAddr,
}

pub struct TimeContext {
    local_offset: UtcOffset,
}
/// # Safety
/// Preconditions:
///  - There shall be no other threads in the process
///
/// Recommend to run this first in main (with no decorators on main, no async executors, etc)
pub unsafe fn get_time_context() -> TimeContext {
    let local_offset = {
        // SAFETY: caller has guaranteed no other threads exist in the process
        unsafe { local_offset::set_soundness(local_offset::Soundness::Unsound) };

        let local_offset = UtcOffset::current_local_offset();

        // SAFETY: called with `Soundness::Sound`
        unsafe { local_offset::set_soundness(local_offset::Soundness::Sound) };

        local_offset.expect("soundness temporarily disabled, to skip thread checks")
    };

    TimeContext { local_offset }
}

impl TimeContext {
    pub fn serve(&self, args: Args) -> anyhow::Result<()> {
        let server = Server::http(args.listen_address).map_err(|e| anyhow::anyhow!(e))?;

        // ensure fail-fast
        {
            let fake_start = Instant::now();
            fmt::format_metrics(self.get_zfs_metrics()?, fake_start);
        }

        loop {
            let request = server.recv()?;
            let start_time = Instant::now();

            let _ = self.handle_request(request, start_time);
        }
    }
    fn handle_request(&self, request: Request, start_time: Instant) -> anyhow::Result<()> {
        const ENDPOINT_METRICS: &str = "/metrics";
        const HTML_NOT_FOUND: u32 = 404;
        let url = request.url();
        if url == ENDPOINT_METRICS {
            let response = self.get_metrics(start_time);
            Ok(request.respond(response)?)
        } else {
            let response = Response::empty(HTML_NOT_FOUND);
            Ok(request.respond(response)?)
        }
    }

    fn get_metrics(&self, start_time: Instant) -> Response<impl std::io::Read> {
        let metrics_str = match self.get_zfs_metrics() {
            Ok(metrics) => fmt::format_metrics(metrics, start_time),
            Err(err) => {
                format!("# ERROR:\n# {err:#}")
            }
        };
        Response::from_string(metrics_str)
    }
}

mod zfs {
    //! Parse the output of ZFS commands
    //!
    //! `PoolMetrics` will contain:
    //! - `None` values if the entry is not present, or
    //! - `Unrecognized` if the entry is present but not a known value
    //!
    //! Novel ZFS errors (a.k.a. unknown to the author) may happen from time to time;
    //! it is crucial to continue reporting metrics in the face of unknown errors/states.
    //!
    //! Therefore, errors are only returned when the input does not match the expected format.
    //! This is a signal that a major format change happened (e.g. requiring updates to this library).

    use crate::TimeContext;
    use anyhow::Context as _;
    use std::{process::Command, str::FromStr};
    use time::{macros::format_description, OffsetDateTime, PrimitiveDateTime};

    pub struct PoolMetrics {
        pub name: String,
        pub state: Option<PoolStatus>,
        pub scan_status: Option<(ScanStatus, OffsetDateTime)>,
        pub devices: Vec<DeviceMetrics>,
        pub error: Option<ErrorStatus>,
    }

    #[derive(Clone, Copy, Debug)]
    pub enum PoolStatus {
        Unrecognized,
        Offline,
        Removed,
        Faulted,
        Split,
        Unavail,
        Degraded,
        Online,
    }
    #[derive(Clone, Copy, Debug)]
    pub enum ScanStatus {
        Unrecognized,
        ScrubRepaired,
    }
    #[derive(Clone, Copy, Debug)]
    pub enum ErrorStatus {
        Unrecognized,
        Ok,
    }

    pub struct DeviceMetrics {
        pub depth: u32,
        pub name: String,
        pub state: String,
        pub errors_read: u32,
        pub errors_write: u32,
        pub errors_checksum: u32,
    }

    #[derive(Default)]
    enum ZpoolStatusSection {
        #[default]
        Header,
        Devices,
    }

    impl TimeContext {
        pub(crate) fn get_zfs_metrics(&self) -> anyhow::Result<Vec<PoolMetrics>> {
            let output = match Command::new("zpool")
                .arg("status")
                .output()
                .map(|output| String::from_utf8(output.stdout))
            {
                Ok(Ok(output)) => output,
                Ok(Err(err)) => anyhow::bail!("{err}"),
                Err(err) => anyhow::bail!("{err}"),
            };

            let mut pools = vec![];
            // disambiguate from header sections and devices (which may contain COLON)
            let mut current_section = ZpoolStatusSection::default();
            for line in output.lines() {
                match current_section {
                    ZpoolStatusSection::Header => {
                        if let Some((label, content)) = line.split_once(':') {
                            let label = label.trim();
                            let content = content.trim();
                            if label == "pool" {
                                let name = content.to_string();
                                pools.push(PoolMetrics::new(name));
                                Ok(())
                            } else if let Some(pool) = pools.last_mut() {
                                pool.parse_line_header(label, content, self)
                            } else {
                                Err(anyhow::anyhow!("missing pool specifier, found header line"))
                            }
                        } else if line.starts_with("\tNAME ") {
                            current_section = ZpoolStatusSection::Devices;
                            Ok(())
                        } else if line.trim().is_empty() {
                            // ignore empty line
                            Ok(())
                        } else {
                            Err(anyhow::anyhow!("unknown line in header"))
                        }
                    }
                    ZpoolStatusSection::Devices => {
                        if line.trim().is_empty() {
                            // ignore empty line, and
                            // back to headers
                            current_section = ZpoolStatusSection::Header;
                            Ok(())
                        } else if let Some(pool) = pools.last_mut() {
                            pool.parse_line_device(line)
                        } else {
                            Err(anyhow::anyhow!("missing pool specifier"))
                        }
                    }
                }
                .with_context(|| format!("on zpool-status output line: {line:?}"))?;
            }
            Ok(pools)
        }
    }

    impl PoolMetrics {
        fn new(name: String) -> Self {
            PoolMetrics {
                name,
                state: None,
                scan_status: None,
                devices: vec![],
                error: None,
            }
        }
        // NOTE: reference the openzfs source for possible formatting changes
        // <https://github.com/openzfs/zfs/blob/6dccdf501ea47bb8a45f00e4904d26efcb917ad4/cmd/zpool/zpool_main.c>
        fn parse_line_header(
            &mut self,
            label: &str,
            content: &str,
            time_context: &TimeContext,
        ) -> anyhow::Result<()> {
            fn err_if_previous(
                (label, content): (&str, &str),
                previous: Option<impl std::fmt::Debug>,
            ) -> anyhow::Result<()> {
                if let Some(previous) = previous {
                    Err(anyhow::anyhow!(
                        "duplicate {label}: {previous:?} and {content:?}"
                    ))
                } else {
                    Ok(())
                }
            }
            match label {
                "state" => {
                    let new_state = content.into();
                    err_if_previous((label, content), self.state.replace(new_state))
                }
                "scan" => {
                    let new_scan_scatus = time_context.parse_scan_content(content)?;
                    err_if_previous((label, content), self.scan_status.replace(new_scan_scatus))
                }
                "config" => {
                    if content.is_empty() {
                        // ignore content
                        Ok(())
                    } else {
                        Err(anyhow::anyhow!(
                            "expected empty content for label {label}, found: {content:?}"
                        ))
                    }
                }
                "errors" => {
                    let new_error = content.into();
                    err_if_previous((label, content), self.error.replace(new_error))
                }
                unknown => Err(anyhow::anyhow!("unknown label: {unknown:?}")),
            }
        }
        fn parse_line_device(&mut self, line: &str) -> anyhow::Result<()> {
            let device = line.parse()?;
            self.devices.push(device);
            Ok(())
        }
    }
    impl TimeContext {
        fn parse_scan_content(
            &self,
            content: &str,
        ) -> anyhow::Result<(ScanStatus, OffsetDateTime)> {
            let Some((message, timestamp)) = content.split_once(" on ") else {
                anyhow::bail!("missing timestamp separator token ON")
            };
            let format = format_description!(
                "[weekday repr:short] [month repr:short] [day] [hour padding:zero repr:24]:[minute]:[second] [year]"
            );
            let scan_status = ScanStatus::from(message);
            let timestamp = PrimitiveDateTime::parse(timestamp, &format)
                .with_context(|| format!("timestamp string {timestamp:?}"))?;
            let timestamp = timestamp.assume_offset(self.local_offset);
            Ok((scan_status, timestamp))
        }
    }

    impl FromStr for DeviceMetrics {
        type Err = anyhow::Error;
        fn from_str(line: &str) -> anyhow::Result<Self> {
            let Some(("", line)) = line.split_once('\t') else {
                anyhow::bail!("malformed device line: {line:?}")
            };
            let (depth, line) = {
                let mut chars = line.chars();
                let mut depth = 0;
                while let Some(' ') = chars.next() {
                    depth += 1;
                }
                let line = &line[depth..];
                let depth =
                    u32::try_from(depth).expect("indentation from human-configurable nesting");
                (depth, line)
            };

            // FIXME - Major assumption: device names will *NOT* have spaces

            let mut cells = line.split_whitespace();
            let Some(name) = cells.next().map(String::from) else {
                anyhow::bail!("missing device name")
            };
            let Some(state) = cells.next().map(String::from) else {
                anyhow::bail!("missing state for device {name:?}")
            };
            let Some(errors_read) = cells.next().map(|s| s.parse()).transpose()? else {
                anyhow::bail!("missing read errors count for device {name:?}")
            };
            let Some(errors_write) = cells.next().map(|s| s.parse()).transpose()? else {
                anyhow::bail!("missing write errors count for device {name:?}")
            };
            let Some(errors_checksum) = cells.next().map(|s| s.parse()).transpose()? else {
                anyhow::bail!("missing checksum errors count for device {name:?}")
            };

            Ok(Self {
                depth,
                name,
                state,
                errors_read,
                errors_write,
                errors_checksum,
            })
        }
    }

    // NOTE: Infallible, so that errors will be shown (reporting service doesn't go down)
    impl From<&str> for ScanStatus {
        fn from(scan_status: &str) -> Self {
            // Key focus: "WHAT" state, and "AS OF WHEN"
            // ignore all other numeric details
            if scan_status.starts_with("scrub repaired") {
                Self::ScrubRepaired
            } else {
                Self::Unrecognized
            }
        }
    }

    // NOTE: Infallible, so that errors will be shown (reporting service doesn't go down)
    //
    // <https://github.com/openzfs/zfs/blob/6dccdf501ea47bb8a45f00e4904d26efcb917ad4/lib/libzfs/libzfs_pool.c#L183>
    impl From<&str> for PoolStatus {
        fn from(scan_status: &str) -> Self {
            match scan_status {
                "OFFLINE" => Self::Offline,
                "REMOVED" => Self::Removed,
                "FAULTED" => Self::Faulted,
                "SPLIT" => Self::Split,
                "UNAVAIL" => Self::Unavail,
                "DEGRADED" => Self::Degraded,
                "ONLINE" => Self::Online,
                _ => Self::Unrecognized,
            }
        }
    }
    // NOTE: Infallible, so that errors will be shown (reporting service doesn't go down)
    impl From<&str> for ErrorStatus {
        fn from(scan_status: &str) -> Self {
            match scan_status {
                "No known data errors" => Self::Ok,
                _ => Self::Unrecognized,
            }
        }
    }
}

mod fmt {
    //! Organize metrics into the prometheus line-by-line format, with comments

    /// Defines the enum with a static field `ALL` containing all variants (in declaration order)
    macro_rules! enum_all {
        (
            $(
                $(#[$meta:meta])*
                $vis:vis enum $name:ident {
                    $(
                        $(#[$meta_inner:meta])*
                        $variant:ident $(= $variant_value:expr)?
                    ),+ $(,)?
                }
            )+
        ) => {
            $(
                $(#[$meta])*
                $vis enum $name {
                    $(
                        $(#[$meta_inner])*
                        $variant $(= $variant_value)?
                    ),+
                }
                impl $name {
                    const ALL: &'static [Self] = &[
                        $(Self::$variant,)+
                    ];
                }
            )+
        };
    }

    /// Defines the enum with:
    /// - `fn summarize_values()` to list the name/value pairs, and
    /// - `fn value()` to retrieve the value
    macro_rules! value_enum {
        (
            $(
                $(#[$meta:meta])*
                $vis:vis enum $name:ident for $source:ident {
                    #[default]
                    UnknownMissing => 0,
                    $(
                        $(#[$meta_inner:meta])*
                        $variant:ident => $variant_value:expr
                    ),+ $(,)?
                }
            )+
        ) => {
            $(
                enum_all! {
                    #[derive(Clone, Copy, Debug, Default)]
                    $(#[$meta])*
                    $vis enum $name {
                        #[default]
                        UnknownMissing = 0,
                        $(
                            $(#[$meta_inner])*
                            $variant = $variant_value
                        ),+
                    }
                }
                impl $name {
                    /// Returns a comma-separated representation of all variants: "Variant = value"
                    pub fn summarize_values() -> impl std::fmt::Display {
                        struct Summary;
                        impl std::fmt::Display for Summary {
                            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                                let mut first = Some(());
                                for &status in $name::ALL {
                                    if first.take().is_none() {
                                        write!(f, ", ")?;
                                    }
                                    let status_num = status.value();
                                    write!(f, "{status:?} = {status_num}")?;
                                }
                                Ok(())
                            }
                        }
                        Summary
                    }
                    pub fn from_opt<T>(source: &Option<T>) -> u32
                    where
                        Self: From<T>,
                        T: Copy,
                    {
                        source.map(Self::from).unwrap_or_default().value()
                    }
                    pub fn value(self) -> u32 {
                        match self {
                            Self::UnknownMissing => 0,
                            $(Self::$variant => $variant_value),+
                        }
                    }
                }
                impl From<$source> for $name {
                    fn from(source: $source) -> Self {
                        match source {
                            $(
                                $source::$variant => Self::$variant
                            ),+
                        }
                    }
                }
                impl<T> From<($source, T)> for $name {
                    fn from((source, _): ($source, T)) -> Self {
                        source.into()
                    }
                }
            )+
        };
    }

    // Define output values
    //
    // Keep the values stable, for continuity in prometheus history
    value_enum! {
        pub enum PoolStateValue for PoolStatus {
            #[default]
            UnknownMissing => 0,
            Unrecognized => 1,
            //
            Online => 2,
            Degraded => 3,
            Faulted  => 4,
            Offline  => 5,
            Removed  => 6,
            Split => 7,
            Unavail  => 8,

        }
        pub enum ScanStatusValue for ScanStatus {
            #[default]
            UnknownMissing => 0,
            Unrecognized => 1,
            //
            ScrubRepaired => 2,
            // TODO Add new statuses here
        }
        pub enum ErrorStatusValue for ErrorStatus {
            #[default]
            UnknownMissing => 0,
            Unrecognized => 1,
            //
            Ok => 2,
            // TODO Add new errors here
        }
    }

    use crate::zfs::{ErrorStatus, PoolMetrics, PoolStatus, ScanStatus};
    use serde::Serialize;
    use std::time::Instant;

    #[derive(Serialize)]
    struct Pool {
        pool: String,
    }
    #[derive(Serialize)]
    struct Device {
        pool: String,
        device: String,
    }

    struct FormatPoolMetrics {
        pools: Vec<PoolMetrics>,
        start_time: Instant,
    }

    pub fn format_metrics(pools: Vec<PoolMetrics>, start_time: Instant) -> String {
        FormatPoolMetrics { pools, start_time }.to_string()
    }

    enum_all! {
        #[derive(Clone, Copy)]
        enum Sections {
            PoolState,
            ScanState,
            ScanAge,
            ErrorState,
        }
    }
    impl std::fmt::Display for FormatPoolMetrics {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            const PREFIX: &str = "zpool_status_export";
            const SECONDS_PER_HOUR: f64 = 60.0 * 60.0;

            let Self { pools, start_time } = self;

            let now = time::OffsetDateTime::now_utc();

            for section in Sections::ALL {
                let metric = match section {
                    Sections::PoolState => {
                        writeln!(f, "# Pool state: {}", PoolStateValue::summarize_values())?;
                        "pool_state"
                    }
                    Sections::ScanState => {
                        writeln!(f, "# Scan status: {}", ScanStatusValue::summarize_values())?;
                        "scan_state"
                    }
                    Sections::ScanAge => {
                        writeln!(f, "# Scan age in hours")?;
                        "scan_age"
                    }
                    Sections::ErrorState => {
                        writeln!(
                            f,
                            "# Error status: {}",
                            ErrorStatusValue::summarize_values()
                        )?;
                        "error_state"
                    }
                };
                for pool in pools {
                    let PoolMetrics {
                        name,
                        state,
                        scan_status,
                        devices: _, // TODO
                        error,
                    } = pool;
                    let value = match section {
                        Sections::PoolState => PoolStateValue::from_opt(state).into(),
                        Sections::ScanState => ScanStatusValue::from_opt(scan_status).into(),
                        Sections::ScanAge => {
                            let seconds = scan_status
                                .as_ref()
                                .map_or(0.0, |&(_, scan_time)| (now - scan_time).as_seconds_f64());
                            seconds / SECONDS_PER_HOUR
                        }
                        Sections::ErrorState => ErrorStatusValue::from_opt(error).into(),
                    };
                    // detect integers to print normally
                    let precision = if value.fract().abs() < f64::EPSILON {
                        // integer
                        0
                    } else {
                        // float
                        6
                    };
                    writeln!(
                        f,
                        "{PREFIX}_{metric}{{pool={name:?}}}={value:.0$}",
                        precision
                    )?;
                }
            }

            writeln!(f, "# total duration of the lookup in microseconds")?;
            let lookup_duration_micros = start_time.elapsed().as_micros();
            writeln!(f, "{PREFIX}_lookup={lookup_duration_micros}")
        }
    }
}
