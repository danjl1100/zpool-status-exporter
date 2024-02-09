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
                let error_str = format!("{err:#}");
                eprintln!("{error_str}");
                error_str
            }
        };
        Response::from_string(metrics_str)
    }
}

mod zfs {
    //! Parse the output of ZFS commands

    use crate::TimeContext;
    use anyhow::Context as _;
    use std::{process::Command, str::FromStr};
    use time::{macros::format_description, OffsetDateTime, PrimitiveDateTime};

    pub struct PoolMetrics {
        pub name: String,
        pub state: Option<String>,
        pub scan_status: Option<(ScanStatus, OffsetDateTime)>,
        pub devices: Vec<DeviceMetrics>,
        pub errors: Vec<String>,
    }
    #[derive(Clone, Copy, Debug)]
    pub enum ScanStatus {
        ScrubRepaired,
    }
    impl ScanStatus {
        const ALL: &'static [Self] = &[Self::ScrubRepaired];
        pub fn summarize_values() -> impl std::fmt::Display {
            ScanStatusSummary
        }
    }
    struct ScanStatusSummary;
    impl std::fmt::Display for ScanStatusSummary {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let mut first = Some(());
            for &status in ScanStatus::ALL {
                if first.take().is_none() {
                    write!(f, ", ")?;
                }
                let status_num = u32::from(status);
                write!(f, "{status:?} = {status_num}")?;
            }
            Ok(())
        }
    }
    impl From<ScanStatus> for u32 {
        fn from(value: ScanStatus) -> Self {
            match value {
                ScanStatus::ScrubRepaired => 1,
            }
        }
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
                .with_context(|| format!("on line {line:?}"))?;
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
                errors: vec![],
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
            match label {
                "state" => {
                    let previous = self.state.replace(content.to_string());
                    if let Some(previous) = previous {
                        anyhow::bail!("duplicate {label}: {previous:?} and {content:?}");
                    }
                }
                "scan" => {
                    let previous = self
                        .scan_status
                        .replace(time_context.parse_scan_content(content)?);
                    if let Some(previous) = previous {
                        anyhow::bail!("duplicate {label}: {previous:?} and {content:?}");
                    }
                }
                "config" => {
                    if !content.is_empty() {
                        anyhow::bail!(
                            "expected empty content for label {label}, found: {content:?}"
                        );
                    }
                }
                "errors" => {
                    if content != "No known data errors" {
                        self.errors.push(content.to_string());
                    }
                }
                unknown => {
                    anyhow::bail!("unknown label: {unknown:?}");
                }
            }
            Ok(())
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
            let scan_status = message.parse()?;
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

    impl FromStr for ScanStatus {
        type Err = anyhow::Error;
        fn from_str(scan_status: &str) -> anyhow::Result<Self> {
            let scan_status = if scan_status.starts_with("scrub repaired") {
                Self::ScrubRepaired
            } else {
                anyhow::bail!("unknown scan status: {scan_status:?}")
            };
            Ok(scan_status)
        }
    }
}

mod fmt {
    //! Organize metrics into the prometheus line-by-line format, with comments

    use crate::zfs::{PoolMetrics, ScanStatus};
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

    #[derive(Clone, Copy)]
    enum Sections {
        ScanState,
        ScanAge,
    }
    impl Sections {
        const ALL: &'static [Self] = &[Self::ScanState, Self::ScanAge];
    }
    impl std::fmt::Display for FormatPoolMetrics {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            const PREFIX: &str = "zpool_status_export";

            let Self { pools, start_time } = self;

            let now = time::OffsetDateTime::now_utc();

            for section in Sections::ALL {
                let metric = match section {
                    Sections::ScanState => {
                        writeln!(f, "# Scan status: {}", ScanStatus::summarize_values())?;
                        "scan_state"
                    }
                    Sections::ScanAge => {
                        writeln!(f, "# Scan age in seconds")?;
                        "scan_age"
                    }
                };
                for pool in pools {
                    let PoolMetrics {
                        name,
                        state,
                        scan_status,
                        devices,
                        errors,
                    } = pool;
                    let value = match section {
                        Sections::ScanState => scan_status
                            .map_or(0, |(scan_status, _)| u32::from(scan_status))
                            .into(),
                        Sections::ScanAge => scan_status
                            .as_ref()
                            .map_or(0, |&(_, scan_time)| (now - scan_time).whole_seconds()),
                    };
                    writeln!(f, "{PREFIX}_{metric}{{pool={name:?}}}={value}")?;
                }
            }

            writeln!(f, "# total duration of the lookup in microseconds")?;
            let lookup_duration_micros = start_time.elapsed().as_micros();
            writeln!(f, "{PREFIX}_lookup={lookup_duration_micros}")
        }
    }
}
