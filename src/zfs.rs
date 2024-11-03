//! Parse the output of ZFS commands, [sans-io](https://sans-io.readthedocs.io/how-to-sans-io.html).
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

use crate::AppContext;
use anyhow::Context as _;
use std::str::FromStr;

#[allow(missing_docs)]
pub struct PoolMetrics {
    pub name: String,
    pub state: Option<DeviceStatus>,
    pub pool_status: Option<PoolStatusDescription>,
    pub scan_status: Option<(ScanStatus, jiff::Zoned)>,
    pub devices: Vec<DeviceMetrics>,
    pub error: Option<ErrorStatus>,
}

#[allow(missing_docs)]
#[derive(Clone, Copy, Debug)]
pub enum DeviceStatus {
    // unknown
    Unrecognized,
    // healthy
    Online,
    // misc
    Offline,
    Split,
    // errors (order by increasing severity)
    Degraded,
    Faulted,
    Suspended, // only for POOL, not VDEV
    Removed,
    Unavail,
}
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug)]
pub enum PoolStatusDescription {
    // unknown
    Unrecognized,
    // healthy
    FeaturesAvailable,
    SufficientReplicasForMissing,
    DeviceRemoved,
    // errors
    DataCorruption,
}
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug)]
pub enum ScanStatus {
    // unknown
    Unrecognized,
    // healthy
    ScrubRepaired,
    Resilvered,
    // misc
    ScrubInProgress,
    // TODO Add new errors here
    // errors
}
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug)]
pub enum ErrorStatus {
    Unrecognized,
    Ok,
    // errors
    DataErrors,
}

/// Numeric metrics for a device
#[derive(Debug)]
pub struct DeviceMetrics {
    /// 0-indexed depth of the device within the device tree
    pub depth: usize,
    /// Device name
    pub name: String,
    /// Device status
    pub state: DeviceStatus,
    /// Count of Read errors
    pub errors_read: u32,
    /// Count of Write errors
    pub errors_write: u32,
    /// Count of Checksum errors
    pub errors_checksum: u32,
}

#[derive(Clone, Copy, Default)]
enum ZpoolStatusSection {
    #[default]
    Header,
    BlankBeforeDevices,
    Devices,
}

impl AppContext {
    /// Extracts discrete metrics from the provided output string (expects `zpool status` format)
    ///
    /// # Errors
    /// Returns an error if the string contains a line that does not match the expected format
    /// (e.g. header line "foobar: ...", or non-numeric error counters in devices list)
    ///
    /// # Notes
    ///
    /// - Any unknown string within the format will be accepted and represented as `Unrecognized`
    ///     (e.g. unknown error message, unknown scan status)
    ///
    /// - Any missing line within the format will result in `None` in the returned struct
    ///     (e.g. no "errors: ..." line or no "scan: ..." line)
    ///
    pub fn parse_zfs_metrics(&self, zpool_output: &str) -> anyhow::Result<Vec<PoolMetrics>> {
        let mut pools = vec![];
        // disambiguate from header sections and devices (which may contain COLON)
        let mut current_section = ZpoolStatusSection::default();
        let mut lines = zpool_output.lines().peekable();
        while let Some(line) = lines.next() {
            // NOTE allocation required for "greedy line append" case in Header
            let mut line = line.to_owned();
            match current_section {
                ZpoolStatusSection::Header => {
                    {
                        // detect line continuations and concatenate
                        while let Some(next_line) = lines.peek() {
                            if let Some(continuation) = next_line.strip_prefix('\t') {
                                line += "\n";
                                line += continuation;
                                lines.next();
                            } else {
                                break;
                            }
                        }
                    }
                    if let Some((label, content)) = line.split_once(':') {
                        let label = label.trim();
                        let content = content.trim();
                        if label == "pool" {
                            let name = content.to_string();
                            pools.push(PoolMetrics::new(name));
                            Ok(())
                        } else if let Some(pool) = pools.last_mut() {
                            let header_result = pool.parse_line_header(label, content, self);

                            if let Ok(Some(next_section)) = &header_result {
                                current_section = *next_section;
                            }
                            header_result.map(|_| ())
                        } else {
                            Err(anyhow::anyhow!("missing pool specifier, found header line"))
                        }
                    } else if line.trim().is_empty() {
                        // ignore empty line
                        Ok(())
                    } else if line == "no pools available" {
                        // ignore marker for "no output"
                        Ok(())
                    } else if line.starts_with("/dev/zfs and /proc/self/mounts") {
                        Err(anyhow::anyhow!(
                            "zpool requires access to /dev/zfs and /proc/self/mounts"
                        ))
                    } else {
                        Err(anyhow::anyhow!("unknown line in header"))
                    }
                }
                ZpoolStatusSection::BlankBeforeDevices => {
                    if line.trim().is_empty() {
                        if let Some(next_line) = lines.peek() {
                            if next_line.starts_with("\tNAME ") {
                                lines.next();
                                current_section = ZpoolStatusSection::Devices;
                                Ok(())
                            } else {
                                Err(anyhow::anyhow!(
                                    "expected device table labels, found: {next_line:?}"
                                ))
                            }
                        } else {
                            Err(anyhow::anyhow!("missing line for device table labels"))
                        }
                    } else {
                        Err(anyhow::anyhow!("expected blank line before devices"))
                    }
                }
                ZpoolStatusSection::Devices => {
                    if !line.starts_with('\t') || line.trim().is_empty() {
                        // end of section - not starting with tab
                        // back to headers
                        current_section = ZpoolStatusSection::Header;
                        Ok(())
                    } else if let Some(pool) = pools.last_mut() {
                        pool.parse_line_device(&line)
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
            pool_status: None,
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
        app_context: &AppContext,
    ) -> anyhow::Result<Option<ZpoolStatusSection>> {
        fn err_if_previous<T>(
            (label, content): (&str, &str),
            previous: Option<impl std::fmt::Debug>,
        ) -> anyhow::Result<Option<T>> {
            if let Some(previous) = previous {
                Err(anyhow::anyhow!(
                    "duplicate {label}: {previous:?} and {content:?}"
                ))
            } else {
                Ok(None)
            }
        }
        match label {
            "status" => {
                // status - a short description of the state
                let new_pool_status = content.into();
                err_if_previous((label, content), self.pool_status.replace(new_pool_status))
            }
            "state" => {
                // state - single token, e.g. DEGRADED, ONLINE
                let new_state = content.into();
                err_if_previous((label, content), self.state.replace(new_state))
            }
            "scan" => {
                let new_scan_status = app_context.parse_scan_content(content)?;
                err_if_previous((label, content), self.scan_status.replace(new_scan_status))
            }
            "config" => {
                // signals empty line prior to devices table
                if content.is_empty() {
                    // ignore content
                    Ok(Some(ZpoolStatusSection::BlankBeforeDevices))
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
            "action" | "see" => {
                // ignore (no metrics)
                Ok(None)
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
impl AppContext {
    fn parse_scan_content(&self, content: &str) -> anyhow::Result<(ScanStatus, jiff::Zoned)> {
        const TIME_SEPARATORS: &[&str] = &[" on ", " since "];

        // remove extra lines - status is only on first line
        let (content, _extra_lines) = content.split_once('\n').unwrap_or((content, ""));

        // extract message and timestamp strings
        let Some((message, timestamp)) = TIME_SEPARATORS
            .iter()
            .find_map(|sep| content.split_once(sep))
        else {
            anyhow::bail!("missing timestamp separator token")
        };

        // parse message
        let scan_status = ScanStatus::from(message);

        // parse timestamp
        let timestamp = self
            .parse_timestamp(timestamp)
            .with_context(|| format!("timestamp string {timestamp:?}"))?;

        Ok((scan_status, timestamp))
    }
    /// Parse a timestamp of this format from zpool status: "Sun Oct 27 15:14:51 2024"
    fn parse_timestamp(&self, timestamp: &str) -> anyhow::Result<jiff::Zoned> {
        let format = "%a %b %d %T %Y";
        let timestamp = jiff::fmt::strtime::BrokenDownTime::parse(format, timestamp)?
            .to_datetime()?
            .to_zoned(self.timezone.clone())?;
        Ok(timestamp)
    }
}

impl FromStr for DeviceMetrics {
    type Err = anyhow::Error;
    fn from_str(line: &str) -> anyhow::Result<Self> {
        // `zpool status` currently uses 2 spaces for each level of indentation
        const DEPTH_MULTIPLE: usize = 2;

        let Some(("", line)) = line.split_once('\t') else {
            anyhow::bail!("malformed device line: {line:?}")
        };
        let (depth, line) = {
            let mut chars = line.chars();
            let mut depth_chars = 0;
            while let Some(' ') = chars.next() {
                depth_chars += 1;
            }
            // NOTE byte indexing via count of chars works because space (' ') is ascii
            let line = &line[depth_chars..];
            let depth = depth_chars / DEPTH_MULTIPLE;
            (depth, line)
        };

        // FIXME - Major assumption: device names will *NOT* have spaces

        let mut cells = line.split_whitespace();
        let Some(name) = cells.next().map(String::from) else {
            anyhow::bail!("missing device name")
        };
        let Some(state) = cells.next().map(DeviceStatus::from) else {
            anyhow::bail!("missing state for device {name:?}")
        };
        let Some(errors_read) = cells
            .next()
            .map(str::parse)
            .transpose()
            .context("read counter")?
        else {
            anyhow::bail!("missing read errors count for device {name:?}")
        };
        let Some(errors_write) = cells
            .next()
            .map(str::parse)
            .transpose()
            .context("write counter")?
        else {
            anyhow::bail!("missing write errors count for device {name:?}")
        };
        let Some(errors_checksum) = cells
            .next()
            .map(str::parse)
            .transpose()
            .context("checksum counter")?
        else {
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
//
// Pool status:
// <https://github.com/openzfs/zfs/blob/6dccdf501ea47bb8a45f00e4904d26efcb917ad4/lib/libzfs/libzfs_pool.c#L247>
//
// ... which may call ...
//
// Device status:
// <https://github.com/openzfs/zfs/blob/6dccdf501ea47bb8a45f00e4904d26efcb917ad4/cmd/zpool/zpool_main.c#L183>
//
impl From<&str> for DeviceStatus {
    fn from(device_status: &str) -> Self {
        match device_status {
            "ONLINE" => Self::Online,
            "OFFLINE" => Self::Offline,
            "SPLIT" => Self::Split,
            "DEGRADED" => Self::Degraded,
            "FAULTED" => Self::Faulted,
            "SUSPENDED" => Self::Suspended,
            "REMOVED" => Self::Removed,
            "UNAVAIL" => Self::Unavail,
            _ => {
                eprintln!("Unrecognized DeviceStatus: {device_status:?}");
                Self::Unrecognized
            }
        }
    }
}

// NOTE: Infallible, so that errors will be shown (reporting service doesn't go down)
impl From<&str> for PoolStatusDescription {
    fn from(pool_status: &str) -> Self {
        // use `concat` macro, because...
        // S.I.C. all line-continuations have "\n\t" removed ("somewords getsmooshed")
        const SUFFICIENT_REPLICAS: &str = concat!(
            "One or more devices could not be used because the label is missing or",
            "\n",
            "invalid.  Sufficient replicas exist for the pool to continue",
            "\n",
            "functioning in a degraded state"
        );
        const DATA_CORRUPTION: &str = concat!(
            "One or more devices has experienced an error resulting in data",
            "\n",
            "corruption.  Applications may be affected"
        );
        const FEATURES_AVAILABLE: &str = concat!(
            "Some supported and requested features are not enabled on the pool.",
            "\n",
            "The pool can still be used, but some features are unavailable.",
        );
        const DEVICE_REMOVED: &str = concat!(
            "One or more devices has been removed by the administrator.",
            "\n",
            "Sufficient replicas exist for the pool to continue functioning in a",
            "\n",
            "degraded state.",
        );
        if pool_status.starts_with(SUFFICIENT_REPLICAS) {
            Self::SufficientReplicasForMissing
        } else if pool_status.starts_with(DATA_CORRUPTION) {
            Self::DataCorruption
        } else if pool_status.starts_with(FEATURES_AVAILABLE) {
            Self::FeaturesAvailable
        } else if pool_status.starts_with(DEVICE_REMOVED) {
            Self::DeviceRemoved
        } else {
            eprintln!("Unrecognized PoolStatusDescription: {pool_status:?}");
            Self::Unrecognized
        }
    }
}

// NOTE: Infallible, so that errors will be shown (reporting service doesn't go down)
impl From<&str> for ScanStatus {
    fn from(scan_status: &str) -> Self {
        // Scan status - only focus on: "WHAT" state (and "AS OF WHEN", elsewhere)
        // ignore all other numeric details
        if scan_status.starts_with("scrub repaired") {
            Self::ScrubRepaired
        } else if scan_status.starts_with("resilvered") {
            Self::Resilvered
        } else if scan_status.starts_with("scrub in progress") {
            Self::ScrubInProgress
        } else {
            eprintln!("Unrecognized ScanStatus: {scan_status:?}");
            Self::Unrecognized
        }
    }
}

// NOTE: Infallible, so that errors will be shown (reporting service doesn't go down)
impl From<&str> for ErrorStatus {
    fn from(error_status: &str) -> Self {
        if error_status.starts_with("No known data errors") {
            Self::Ok
        } else {
            let (_first_word, remainder) =
                error_status.split_once(' ').unwrap_or((error_status, ""));
            if remainder.starts_with("data errors") {
                Self::DataErrors
            } else {
                eprintln!("Unrecognized ErrorStatus: {error_status:?}");
                Self::Unrecognized
            }
        }
    }
}
