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

use crate::TimeContext;
use anyhow::Context as _;
use std::str::FromStr;
use time::{macros::format_description, OffsetDateTime, PrimitiveDateTime};

#[allow(missing_docs)]
pub struct PoolMetrics {
    pub name: String,
    pub state: Option<DeviceStatus>,
    pub scan_status: Option<(ScanStatus, OffsetDateTime)>,
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
pub enum ScanStatus {
    Unrecognized,
    ScrubRepaired,
}
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug)]
pub enum ErrorStatus {
    Unrecognized,
    Ok,
}

/// Numeric metrics for a device
pub struct DeviceMetrics {
    /// 0-indexed depth of the device within the device tree
    pub depth: u32,
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

#[derive(Default)]
enum ZpoolStatusSection {
    #[default]
    Header,
    Devices,
}

impl TimeContext {
    /// Extracts discrete metrics from the provided output string (expects `zpool status` format)
    ///
    /// # Errors
    /// Returns an error if the string contains a line that does not match the expected format
    /// (e.g. header line "foobar: ...", or non-numeric error counters in devices list)
    ///
    /// # Notes
    ///
    /// - Any unknown string within the format will be accepted and represented as `Unrecognized`
    /// (e.g. unknown error message, unknown scan status)
    ///
    /// - Any missing line within the format will result in `None` in the returned struct
    /// (e.g. no "errors: ..." line or no "scan: ..." line)
    ///
    pub fn parse_zfs_metrics(&self, zpool_output: &str) -> anyhow::Result<Vec<PoolMetrics>> {
        let mut pools = vec![];
        // disambiguate from header sections and devices (which may contain COLON)
        let mut current_section = ZpoolStatusSection::default();
        for line in zpool_output.lines() {
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
    fn parse_scan_content(&self, content: &str) -> anyhow::Result<(ScanStatus, OffsetDateTime)> {
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
            let depth = u32::try_from(depth).expect("indentation from human-configurable nesting");
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
// Pool status:
// <https://github.com/openzfs/zfs/blob/6dccdf501ea47bb8a45f00e4904d26efcb917ad4/lib/libzfs/libzfs_pool.c#L247>
//
// ... which may call ...
//
// Device status:
// <https://github.com/openzfs/zfs/blob/6dccdf501ea47bb8a45f00e4904d26efcb917ad4/cmd/zpool/zpool_main.c#L183>
//
impl From<&str> for DeviceStatus {
    fn from(scan_status: &str) -> Self {
        match scan_status {
            "ONLINE" => Self::Online,
            "OFFLINE" => Self::Offline,
            "SPLIT" => Self::Split,
            "DEGRADED" => Self::Degraded,
            "FAULTED" => Self::Faulted,
            "SUSPENDED" => Self::Suspended,
            "REMOVED" => Self::Removed,
            "UNAVAIL" => Self::Unavail,
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
