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

pub use main::Error as ParseError;

#[allow(missing_docs)]
pub(crate) struct PoolMetrics {
    pub name: String,
    pub state: Option<DeviceStatus>,
    pub pool_status: Option<PoolStatusDescription>,
    pub scan_status: Option<(ScanStatus, jiff::Zoned)>,
    pub devices: Vec<DeviceMetrics>,
    pub error: Option<ErrorStatus>,
}

#[allow(missing_docs)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum DeviceStatus {
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
pub(super) enum PoolStatusDescription {
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
pub(super) enum ScanStatus {
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
pub(super) enum ErrorStatus {
    Unrecognized,
    Ok,
    // errors
    DataErrors,
}

/// Numeric metrics for a device
#[derive(Debug)]
pub(super) struct DeviceMetrics {
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

#[derive(Clone, Copy, Default, Debug)]
enum ZpoolStatusSection {
    #[default]
    Header,
    BlankBeforeDevices,
    Devices,
}

mod main {
    use super::{device_metrics, metrics_line_header, PoolMetrics, ZpoolStatusSection};
    use crate::AppContext;

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
        ///   (e.g. unknown error message, unknown scan status)
        ///
        /// - Any missing line within the format will result in `None` in the returned struct
        ///   (e.g. no "errors: ..." line or no "scan: ..." line)
        ///
        pub(crate) fn parse_zfs_metrics(
            &self,
            zpool_output: &str,
        ) -> Result<Vec<PoolMetrics>, Error> {
            let mut pools = vec![];
            // disambiguate from header sections and devices (which may contain COLON)
            let mut current_section = ZpoolStatusSection::default();
            let mut lines = zpool_output.lines().enumerate().peekable();
            while let Some((line_index, line)) = lines.next() {
                // NOTE allocation required for "greedy line append" case in Header
                // TODO: Cow? to delay allocation until the continuation actually happens
                let make_error = |kind| Error {
                    line: line.to_owned(),
                    line_number: line_index + 1,
                    kind,
                };
                let mut line = line.to_owned();
                match current_section {
                    ZpoolStatusSection::Header => {
                        {
                            // detect line continuations and concatenate
                            while let Some((_index, next_line)) = lines.peek() {
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
                                let header_result = pool.add_line_header(label, content, self);

                                if let Ok(Some(next_section)) = &header_result {
                                    current_section = *next_section;
                                }
                                Ok(header_result
                                    .map(|_| ())
                                    .map_err(ErrorKind::MetricsLineHeader)
                                    .map_err(make_error)?)
                            } else {
                                Err(make_error(ErrorKind::HeaderBeforePool {
                                    label: label.to_owned(),
                                }))
                            }
                        } else if line.trim().is_empty() {
                            // ignore empty line
                            Ok(())
                        } else if line == "no pools available" {
                            // ignore marker for "no output"
                            Ok(())
                        } else if line.starts_with("/dev/zfs and /proc/self/mounts") {
                            Err(make_error(ErrorKind::NeedsZfsDeviceMounts))
                        } else {
                            Err(make_error(ErrorKind::UnknownHeader))
                        }
                    }
                    ZpoolStatusSection::BlankBeforeDevices => {
                        if line.trim().is_empty() {
                            if let Some((_index, next_line)) = lines.peek() {
                                if next_line.starts_with("\tNAME ") {
                                    lines.next();
                                    current_section = ZpoolStatusSection::Devices;
                                    Ok(())
                                } else {
                                    Err(make_error(ErrorKind::InvalidDeviceTableLabels))
                                }
                            } else {
                                Err(make_error(ErrorKind::MissingDeviceTableLabels))
                            }
                        } else {
                            Err(make_error(ErrorKind::MissingBlankForDevices))
                        }
                    }
                    ZpoolStatusSection::Devices => {
                        let is_table_row = line.starts_with('\t');
                        let is_empty = line.trim().is_empty();
                        if !is_table_row || is_empty {
                            if !is_empty {
                                eprintln!("ignoring line interrupting devices table: {line:?}");
                            }

                            // end of section - not starting with tab
                            // back to headers
                            current_section = ZpoolStatusSection::Header;
                            Ok(())
                        } else if let Some(pool) = pools.last_mut() {
                            Ok(pool
                                .parse_line_device(&line)
                                .map_err(ErrorKind::DeviceMetrics)
                                .map_err(make_error)?)
                        } else {
                            unreachable!(
                                "{current_section:?} should not be active while `pools` is empty"
                            )
                        }
                    }
                }?;
            }
            Ok(pools)
        }
    }

    /// Error parsing the output from the `zpool status` command
    #[derive(Debug)]
    pub struct Error {
        line: String,
        line_number: usize,
        kind: ErrorKind,
    }
    #[derive(Debug)]
    enum ErrorKind {
        MetricsLineHeader(metrics_line_header::Error),
        DeviceMetrics(device_metrics::Error),
        HeaderBeforePool { label: String },
        NeedsZfsDeviceMounts,
        UnknownHeader,
        InvalidDeviceTableLabels,
        MissingDeviceTableLabels,
        MissingBlankForDevices,
    }
    impl std::error::Error for Error {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match &self.kind {
                ErrorKind::MetricsLineHeader(error) => Some(error),
                ErrorKind::DeviceMetrics(error) => Some(error),
                ErrorKind::HeaderBeforePool { label: _ }
                | ErrorKind::NeedsZfsDeviceMounts
                | ErrorKind::UnknownHeader
                | ErrorKind::InvalidDeviceTableLabels
                | ErrorKind::MissingDeviceTableLabels
                | ErrorKind::MissingBlankForDevices => None,
            }
        }
    }
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self {
                line,
                line_number,
                kind,
            } = self;
            match kind {
                ErrorKind::MetricsLineHeader(_error) => write!(f, "unexpected metrics header"),
                ErrorKind::DeviceMetrics(_error) => write!(f, "unexpected device metrics"),
                ErrorKind::HeaderBeforePool { label } => {
                    write!(f, "unexpected header {label:?} before pool label")
                }
                ErrorKind::NeedsZfsDeviceMounts => {
                    write!(f, "zpool requires access to /dev/zfs and /proc/self/mounts")
                }
                ErrorKind::UnknownHeader => write!(f, "unknown header"),
                ErrorKind::InvalidDeviceTableLabels => {
                    write!(f, "invalid device table labels")
                }
                ErrorKind::MissingDeviceTableLabels => write!(f, "missing device table labels"),
                ErrorKind::MissingBlankForDevices => write!(f, "expect blank line before devices"),
            }?;
            write!(f, " on zpool-status output line {line_number}: {line:?}")
        }
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
    fn parse_line_device(&mut self, line: &str) -> Result<(), device_metrics::Error> {
        let device = line.parse()?;
        self.devices.push(device);
        Ok(())
    }
}

mod metrics_line_header {
    use super::{PoolMetrics, ZpoolStatusSection};
    use crate::AppContext;
    impl PoolMetrics {
        // NOTE: reference the openzfs source for possible formatting changes
        // <https://github.com/openzfs/zfs/blob/6dccdf501ea47bb8a45f00e4904d26efcb917ad4/cmd/zpool/zpool_main.c>
        pub(super) fn add_line_header(
            &mut self,
            label: &str,
            content: &str,
            app_context: &AppContext,
        ) -> Result<Option<ZpoolStatusSection>, Error> {
            fn err_if_previous<T>(
                previous: Option<impl std::fmt::Debug + 'static>,
            ) -> Result<Option<T>, ErrorKind> {
                if let Some(previous) = previous {
                    Err(ErrorKind::DuplicateEntry {
                        previous: format!("{previous:?}"),
                    })
                } else {
                    Ok(None)
                }
            }
            let make_error = |kind| Error {
                label: label.to_owned(),
                content: content.to_owned(),
                kind,
            };
            match label {
                "status" => {
                    // status - a short description of the state
                    let new_pool_status = content.into();
                    err_if_previous(self.pool_status.replace(new_pool_status)).map_err(make_error)
                }
                "state" => {
                    // state - single token, e.g. DEGRADED, ONLINE
                    let new_state = content.into();
                    err_if_previous(self.state.replace(new_state)).map_err(make_error)
                }
                "scan" => {
                    let new_scan_status = app_context
                        .parse_scan_content(content)
                        .map_err(ErrorKind::ScanContent)
                        .map_err(make_error)?;
                    err_if_previous(self.scan_status.replace(new_scan_status)).map_err(make_error)
                }
                "config" => {
                    // signals empty line prior to devices table
                    if content.is_empty() {
                        // ignore content
                        Ok(Some(ZpoolStatusSection::BlankBeforeDevices))
                    } else {
                        Err(make_error(ErrorKind::ExpectedEmpty))
                    }
                }
                "errors" => {
                    let new_error = content.into();
                    err_if_previous(self.error.replace(new_error)).map_err(make_error)
                }
                "action" | "see" => {
                    // ignore (no metrics)
                    Ok(None)
                }
                _ => Err(make_error(ErrorKind::UnknownLabel)),
            }
        }
    }

    #[derive(Debug)]
    pub(super) struct Error {
        label: String,
        content: String,
        kind: ErrorKind,
    }
    #[derive(Debug)]
    enum ErrorKind {
        DuplicateEntry { previous: String },
        ScanContent(super::scan_content::Error),
        ExpectedEmpty,
        UnknownLabel,
    }
    impl std::error::Error for Error {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match &self.kind {
                ErrorKind::DuplicateEntry { .. }
                | ErrorKind::ExpectedEmpty
                | ErrorKind::UnknownLabel => None,
                ErrorKind::ScanContent(err) => Some(err),
            }
        }
    }
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self {
                label,
                content,
                kind,
            } = self;
            match kind {
                ErrorKind::DuplicateEntry { previous } => {
                    write!(f, "duplicate {label}: {previous:?} and {content:?}")
                }
                ErrorKind::ScanContent(_) => write!(f, "invalid {label} content {content:?}"),
                ErrorKind::ExpectedEmpty => {
                    write!(f, "expected empty line for {label}, found {content:?}")
                }
                ErrorKind::UnknownLabel => {
                    write!(f, "unknown label {label:?} with content {content:?}")
                }
            }
        }
    }
}

mod scan_content {
    use crate::{zfs::ScanStatus, AppContext};

    const TIME_SEPARATORS: &[&str] = &[" on ", " since "];

    impl AppContext {
        pub(super) fn parse_scan_content(
            &self,
            content: &str,
        ) -> Result<(ScanStatus, jiff::Zoned), Error> {
            // remove extra lines - status is only on first line
            let (content, _extra_lines) = content.split_once('\n').unwrap_or((content, ""));

            let make_error = |kind| Error {
                // scan_content: content.to_owned(),
                kind,
            };

            // extract message and timestamp strings
            let (message, timestamp) = TIME_SEPARATORS
                .iter()
                .find_map(|sep| content.split_once(sep))
                .ok_or(ErrorKind::MissingTimestampSeparator)
                .map_err(make_error)?;

            // parse message
            let scan_status = ScanStatus::from(message);

            // parse timestamp
            let timestamp = self
                .parse_timestamp(timestamp)
                .map_err(|err| {
                    let timestamp = timestamp.to_owned();
                    ErrorKind::ParseTimestamp { timestamp, err }
                })
                .map_err(make_error)?;

            Ok((scan_status, timestamp))
        }
        /// Parse a timestamp of this format from zpool status: "Sun Oct 27 15:14:51 2024"
        fn parse_timestamp(&self, timestamp: &str) -> Result<jiff::Zoned, jiff::Error> {
            let format = "%a %b %d %T %Y";
            let timestamp = jiff::fmt::strtime::BrokenDownTime::parse(format, timestamp)?
                .to_datetime()?
                .to_zoned(self.timezone.clone())?;
            Ok(timestamp)
        }
    }

    #[derive(Debug)]
    pub(super) struct Error {
        // scan_content: String,
        kind: ErrorKind,
    }
    #[derive(Debug)]
    enum ErrorKind {
        MissingTimestampSeparator,
        ParseTimestamp { timestamp: String, err: jiff::Error },
    }
    impl std::error::Error for Error {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match &self.kind {
                ErrorKind::MissingTimestampSeparator => None,
                ErrorKind::ParseTimestamp { err, .. } => Some(err),
            }
        }
    }
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self { kind } = self;
            match kind {
                ErrorKind::MissingTimestampSeparator => {
                    write!(
                        f,
                        "expected timestamp separator token (one of {TIME_SEPARATORS:?})"
                    )
                }
                ErrorKind::ParseTimestamp { timestamp, err: _ } => {
                    write!(f, "invalid timestamp {timestamp:?}")
                }
            }
            // write!(f, " in scan content {scan_content:?}")
        }
    }
}

mod device_metrics {
    use super::DeviceMetrics;
    use crate::zfs::DeviceStatus;
    use std::str::FromStr;

    impl FromStr for DeviceMetrics {
        type Err = Error;
        fn from_str(line: &str) -> Result<Self, Error> {
            // `zpool status` currently uses 2 spaces for each level of indentation
            const DEPTH_MULTIPLE: usize = 2;

            let make_error = |kind| Error {
                device_name: None,
                kind,
            };

            let (before_tab, line) = line
                .split_once('\t')
                .ok_or(ErrorKind::MissingLeadingWhitespace)
                .map_err(make_error)?;
            if !before_tab.is_empty() {
                return Err(make_error(ErrorKind::InvalidLeadingWhitespace));
            }

            let (depth, line) = {
                let mut chars = line.chars();
                let mut depth_chars = 0;
                while let Some(' ') = chars.next() {
                    depth_chars += 1;
                }
                // NOTE byte indexing via count of chars only works because space (' ') is ascii
                let line = &line[depth_chars..];
                let depth = depth_chars / DEPTH_MULTIPLE;
                (depth, line)
            };

            // FIXME - Major assumption: device names will *NOT* have spaces

            let mut cells = line.split_whitespace();
            let name = cells
                .next()
                .map(String::from)
                .ok_or(ErrorKind::MissingName)
                .map_err(make_error)?;

            let make_error = |kind| Error {
                device_name: Some(name.clone()),
                kind,
            };
            let parse_count = |cell: Option<&str>, kind_if_missing| {
                cell.ok_or(kind_if_missing)
                    .and_then(|cell| {
                        cell.parse().map_err(|error| ErrorKind::InvalidCount {
                            error,
                            cell: cell.to_owned(),
                        })
                    })
                    .map_err(make_error)
            };

            let state = cells
                .next()
                .map(DeviceStatus::from)
                .ok_or(ErrorKind::MissingState)
                .map_err(make_error)?;
            let errors_read = parse_count(cells.next(), ErrorKind::MissingReadErrorCount)?;
            let errors_write = parse_count(cells.next(), ErrorKind::MissingWriteErrorCount)?;
            let errors_checksum = parse_count(cells.next(), ErrorKind::MissingChecksumErrorCount)?;

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

    #[derive(Debug)]
    pub(crate) struct Error {
        device_name: Option<String>,
        kind: ErrorKind,
    }
    #[derive(Debug)]
    enum ErrorKind {
        MissingLeadingWhitespace,
        MissingName,
        MissingState,
        MissingReadErrorCount,
        MissingWriteErrorCount,
        MissingChecksumErrorCount,
        InvalidLeadingWhitespace,
        InvalidCount {
            error: std::num::ParseIntError,
            cell: String,
        },
    }
    impl std::error::Error for Error {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            match &self.kind {
                ErrorKind::MissingLeadingWhitespace
                | ErrorKind::MissingName
                | ErrorKind::MissingState
                | ErrorKind::MissingReadErrorCount
                | ErrorKind::MissingWriteErrorCount
                | ErrorKind::MissingChecksumErrorCount
                | ErrorKind::InvalidLeadingWhitespace => None,
                ErrorKind::InvalidCount { error, .. } => Some(error),
            }
        }
    }
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self { device_name, kind } = self;
            let description = match kind {
                ErrorKind::MissingLeadingWhitespace => "expected leading table whitespace",
                ErrorKind::MissingName => "expected device name",
                ErrorKind::MissingState => "expected device state",
                ErrorKind::MissingReadErrorCount => "expected read error count",
                ErrorKind::MissingWriteErrorCount => "expected write error count",
                ErrorKind::MissingChecksumErrorCount => "expected checksum error count",
                ErrorKind::InvalidLeadingWhitespace => "invalid leading whitespace in table",
                ErrorKind::InvalidCount { error: _, cell } => &format!("invalid count {cell:?}"),
            };
            if let Some(device_name) = device_name {
                write!(f, "{description} for device {device_name:?}")
            } else {
                write!(f, "{description}")
            }
        }
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
        const FEATURES_AVAILABLE: &[&str] = &[
            concat!(
                "Some supported and requested features are not enabled on the pool.",
                "\n",
                "The pool can still be used, but some features are unavailable.",
            ),
            concat!(
                "Some supported features are not enabled on the pool. The pool can",
                "\n",
                "still be used, but some features are unavailable.",
            ),
        ];
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
        } else if FEATURES_AVAILABLE
            .iter()
            .any(|pattern| pool_status.starts_with(pattern))
        {
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
