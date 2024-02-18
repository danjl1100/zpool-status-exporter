//! Organize metrics into the prometheus line-by-line format, with comments

#[macro_use]
mod macros;

// Define output values
//
// Keep the values stable, for continuity in prometheus history
value_enum! {
    #[allow(missing_docs)]
    pub enum DeviceStatusValue for DeviceStatus {
        #[default]
        UnknownMissing => 0,
        Unrecognized => 1,
        // healthy
        Online => 10,
        // misc
        Offline => 25,
        Split => 26,
        // errors (order by increasing severity)
        Degraded => 50,
        Faulted  => 60,
        Suspended  => 70,
        Removed => 80,
        Unavail  => 100,
    }
    #[allow(missing_docs)]
    pub enum PoolStatusDescriptionValue for PoolStatusDescription {
        #[default]
        Normal => 0,
        Unrecognized => 1,
        // normal
        FeaturesAvailable => 5,
        SufficientReplicasForMissing => 10,
        // errors
        DataCorruption => 50,
    }
    #[allow(missing_docs)]
    pub enum ScanStatusValue for ScanStatus {
        #[default]
        UnknownMissing => 0,
        Unrecognized => 1,
        // healthy
        ScrubRepaired => 10,
        Resilvered => 15,
        // misc
        ScrubInProgress => 30,
        // errors
        // TODO Add new statuses here
    }
    #[allow(missing_docs)]
    pub enum ErrorStatusValue for ErrorStatus {
        #[default]
        UnknownMissing => 0,
        Unrecognized => 1,
        // healthy
        Ok => 10,
        // errors
        DataErrors => 50,
    }
}

use crate::zfs::{
    DeviceMetrics, DeviceStatus, ErrorStatus, PoolMetrics, PoolStatusDescription, ScanStatus,
};
use std::time::Instant;

struct FormatPoolMetrics {
    pools: Vec<PoolMetrics>,
    now: time::OffsetDateTime,
    /// If present, start time for the computation
    ///
    /// When not provided, no duration will be reported
    compute_time_start: Option<Instant>,
}

/// Returns the "prometheus style" output metrics for the specified `pools`
#[must_use]
pub fn format_metrics(
    pools: Vec<PoolMetrics>,
    now: time::OffsetDateTime,
    compute_time_start: Option<Instant>,
) -> String {
    FormatPoolMetrics {
        pools,
        now,
        compute_time_start,
    }
    .to_string()
}
const PREFIX: &str = "zpool_status_export";

impl std::fmt::Display for FormatPoolMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.pools.is_empty() {
            writeln!(f, "# no pools reported")?;
        } else {
            self.fmt_pool_sections(f)?;

            self.fmt_device_sections(f)?;
        }

        if let Some(start_time) = self.compute_time_start {
            writeln!(f, "# total duration of the lookup in microseconds")?;
            let lookup_duration_micros = start_time.elapsed().as_micros();
            writeln!(f, "{PREFIX}_lookup={lookup_duration_micros}")?;
        }
        Ok(())
    }
}

enum_all! {
    #[derive(Clone, Copy)]
    enum PoolSections {
        PoolState,
        PoolStatusDescription,
        ScanState,
        ScanAge,
        ErrorState,
    }
}
impl FormatPoolMetrics {
    fn fmt_pool_sections(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const SECONDS_PER_HOUR: f64 = 60.0 * 60.0;

        use PoolSections as S;
        for section in S::ALL {
            let metric = match section {
                S::PoolState => {
                    writeln!(f, "# Pool state: {}", DeviceStatusValue::summarize_values())?;
                    "pool_state"
                }
                S::PoolStatusDescription => {
                    writeln!(
                        f,
                        "# Pool status description: {}",
                        PoolStatusDescriptionValue::summarize_values()
                    )?;
                    "pool_status_desc"
                }
                S::ScanState => {
                    writeln!(f, "# Scan status: {}", ScanStatusValue::summarize_values())?;
                    "scan_state"
                }
                S::ScanAge => {
                    writeln!(f, "# Scan age in hours")?;
                    "scan_age"
                }
                S::ErrorState => {
                    writeln!(
                        f,
                        "# Error status: {}",
                        ErrorStatusValue::summarize_values()
                    )?;
                    "error_state"
                }
            };
            for pool in &self.pools {
                let PoolMetrics {
                    name,
                    state,
                    pool_status,
                    scan_status,
                    devices: _, // see `fmt_device_sections`
                    error,
                } = pool;
                let value = match section {
                    S::PoolState => DeviceStatusValue::from_opt(state).into(),
                    S::PoolStatusDescription => {
                        PoolStatusDescriptionValue::from_opt(pool_status).into()
                    }
                    S::ScanState => ScanStatusValue::from_opt(scan_status).into(),
                    S::ScanAge => {
                        let seconds = scan_status.as_ref().map_or(0.0, |&(_, scan_time)| {
                            (self.now - scan_time).as_seconds_f64()
                        });
                        seconds / SECONDS_PER_HOUR
                    }
                    S::ErrorState => ErrorStatusValue::from_opt(error).into(),
                };
                // detect integers to print normally
                let precision = if value.fract().abs() < f64::EPSILON {
                    // integer
                    0
                } else {
                    // float
                    6
                };
                writeln!(f, "{PREFIX}_{metric}{{pool={name:?}}}={value:.precision$}")?;
            }
        }
        Ok(())
    }
}

enum_all! {
    #[derive(Clone, Copy)]
    enum DeviceSections {
        State,
        ErrorsRead,
        ErrorsWrite,
        ErrorsChecksum,
    }
}
impl FormatPoolMetrics {
    fn fmt_device_sections(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use DeviceSections as S;
        for section in S::ALL {
            let metric = match section {
                S::State => {
                    writeln!(
                        f,
                        "# Device state: {}",
                        DeviceStatusValue::summarize_values()
                    )?;
                    "dev_state"
                }
                S::ErrorsRead => {
                    writeln!(f, "# Read error count")?;
                    "dev_errors_read"
                }
                S::ErrorsWrite => {
                    writeln!(f, "# Write error count")?;
                    "dev_errors_write"
                }
                S::ErrorsChecksum => {
                    writeln!(f, "# Checksum error count")?;
                    "dev_errors_checksum"
                }
            };
            for pool in &self.pools {
                let pool_name = &pool.name;

                let mut dev_name = DeviceTreeName::default();
                for device in &pool.devices {
                    let DeviceMetrics {
                        depth,
                        ref name,
                        state,
                        errors_read,
                        errors_write,
                        errors_checksum,
                    } = *device;
                    dev_name.update(depth, name.clone());
                    let value = match section {
                        S::State => DeviceStatusValue::from(state).value(),
                        S::ErrorsRead => errors_read,
                        S::ErrorsWrite => errors_write,
                        S::ErrorsChecksum => errors_checksum,
                    };
                    writeln!(
                        f,
                        "{PREFIX}_{metric}{{pool={pool_name:?},dev={dev_name:?}}}={value}"
                    )?;
                }
            }
        }
        Ok(())
    }
}

/// Helper for printing device tree elements as slash/separated/strings
///
/// NOTE: The `Debug` implementation surrounds the output in quotes, to match the `String` behavior
#[derive(Default)]
struct DeviceTreeName(Vec<String>);
impl DeviceTreeName {
    fn update(&mut self, depth: usize, name: String) {
        // exclude the "depth=0" element (pool name)
        let Some(depth) = depth.checked_sub(1) else {
            self.0.clear();
            return;
        };
        self.0.truncate(depth);
        self.0.push(name);
    }
}
impl std::fmt::Debug for DeviceTreeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\"")?;
        let mut first = Some(());
        for elem in &self.0 {
            if first.take().is_none() {
                write!(f, "/")?;
            }
            write!(f, "{elem}")?;
        }
        write!(f, "\"")
    }
}
