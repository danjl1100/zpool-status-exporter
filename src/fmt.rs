//! Organize metrics into the prometheus line-by-line format, with comments

#[macro_use]
mod macros;

mod meta;

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
        DeviceRemoved => 15,
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

use self::context::WriteKeyValue as _;
use crate::{
    fmt::meta::MetricWrite as _,
    zfs::{
        DeviceMetrics, DeviceStatus, ErrorStatus, PoolMetrics, PoolStatusDescription, ScanStatus,
    },
};
use std::time::Instant;

struct FormatPoolMetrics {
    pools: Vec<PoolMetrics>,
    now: time::OffsetDateTime,
    now_jiff: jiff::Zoned,
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
    now_jiff: jiff::Zoned,
    compute_time_start: Option<Instant>,
) -> String {
    FormatPoolMetrics {
        pools,
        now,
        now_jiff,
        compute_time_start,
    }
    .to_string()
}

mod context {
    pub fn write_prefix_label<T: super::meta::MetricWrite + ?Sized>(
        key: &T,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        const PREFIX: &str = "zpool";
        let key = key.metric_name();
        write!(f, "{PREFIX}_{key}")
    }

    pub trait WriteKeyValue {
        fn write_kv<T: super::meta::MetricWrite + ?Sized>(
            &self,
            f: &mut std::fmt::Formatter<'_>,
            key: &T,
            value: f64,
        ) -> std::fmt::Result {
            write_prefix_label(key, f)?;

            self.fmt_context(f)?;

            // detect integers to print normally
            let precision = if value.fract().abs() < f64::EPSILON {
                // integer
                0
            } else {
                // float
                6
            };
            writeln!(f, " {value:.precision$}")
        }
        fn fmt_context(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result;
    }
    pub struct Empty;
    impl WriteKeyValue for Empty {
        fn fmt_context(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            Ok(())
        }
    }
    pub struct Pool<'a> {
        pub pool_name: &'a str,
    }
    impl WriteKeyValue for Pool<'_> {
        fn fmt_context(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self { pool_name } = self;
            write!(f, "{{pool={pool_name:?}}}")
        }
    }
    pub struct Device<'a> {
        pub pool_name: &'a str,
        pub dev_name: &'a super::DeviceTreeName,
    }
    impl WriteKeyValue for Device<'_> {
        fn fmt_context(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self {
                pool_name,
                dev_name,
            } = self;
            write!(f, "{{pool={pool_name:?},dev={dev_name:?}}}")
        }
    }
}

impl std::fmt::Display for FormatPoolMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.pools.is_empty() {
            writeln!(f, "# no pools reported")?;
        } else {
            self.fmt_pool_sections(f)?;

            self.fmt_device_sections(f)?;
        }

        if let Some(start_time) = self.compute_time_start {
            const LOOKUP: meta::SimpleMetric =
                meta::metric("lookup", "total duration of the lookup in seconds");
            LOOKUP.write_meta(f)?;
            let lookup_duration = start_time.elapsed().as_secs_f64();
            context::Empty.write_kv(f, &LOOKUP, lookup_duration)?;
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
        const POOL_STATE: meta::ValuesMetric<DeviceStatusValue> =
            meta::metric("pool_state", "Pool state").with_values();
        const POOL_STATUS_DESCRIPTION: meta::ValuesMetric<PoolStatusDescriptionValue> =
            meta::metric("pool_status_desc", "Pool status description").with_values();
        const SCAN_STATE: meta::ValuesMetric<ScanStatusValue> = //
            meta::metric("scan_state", "Scan status").with_values();
        const SCAN_AGE: meta::SimpleMetric = //
            meta::metric("scan_age", "Scan age in hours");
        const ERROR_STATE: meta::ValuesMetric<ErrorStatusValue> =
            meta::metric("error_state", "Error status").with_values();

        const SECONDS_PER_HOUR: f64 = 60.0 * 60.0;

        use PoolSections as S;
        for section in S::ALL {
            let metric: &dyn meta::MetricWrite = match section {
                S::PoolState => &POOL_STATE,
                S::PoolStatusDescription => &POOL_STATUS_DESCRIPTION,
                S::ScanState => &SCAN_STATE,
                S::ScanAge => &SCAN_AGE,
                S::ErrorState => &ERROR_STATE,
            };
            metric.write_meta(f)?;

            for pool in &self.pools {
                let PoolMetrics {
                    name: pool_name,
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
                        let seconds = scan_status.as_ref().map_or(
                            0.0,
                            |(_, (scan_time_old, scan_time_jiff))| {
                                let seconds_old = (self.now - *scan_time_old).as_seconds_f64();
                                if false {
                                    // assert that `jiff` gets the same result
                                    let seconds_jiff = (&self.now_jiff - scan_time_jiff)
                                        .total(jiff::Unit::Second)
                                        .expect("no overflow and relative zoned");
                                    let seconds_error = seconds_jiff - seconds_old;
                                    assert!(
                                        seconds_error.abs() < 0.01,
                                        "difference jiff - old = {seconds_error}\n\told {self_now} - {scan_time_old} = {seconds_old}\n\tjiff {self_now_jiff} - {scan_time_jiff} = {seconds_jiff}",
                                        self_now = self.now,
                                        self_now_jiff = self.now_jiff,
                                    );
                                }
                                seconds_old
                            },
                        );
                        seconds / SECONDS_PER_HOUR
                    }
                    S::ErrorState => ErrorStatusValue::from_opt(error).into(),
                };
                context::Pool { pool_name }.write_kv(f, metric, value)?;
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
        const DEVICE_STATE: meta::ValuesMetric<DeviceStatusValue> =
            meta::metric("dev_state", "Device state").with_values();
        const ERRORS_READ: meta::SimpleMetric = //
            meta::metric("dev_errors_read", "Read error count");
        const ERRORS_WRITE: meta::SimpleMetric = //
            meta::metric("dev_errors_write", "Write error count");
        const ERRORS_CHECKSUM: meta::SimpleMetric = //
            meta::metric("dev_errors_checksum", "Checksum error count");

        use DeviceSections as S;
        for section in S::ALL {
            let metric: &dyn meta::MetricWrite = match section {
                S::State => &DEVICE_STATE,
                S::ErrorsRead => &ERRORS_READ,
                S::ErrorsWrite => &ERRORS_WRITE,
                S::ErrorsChecksum => &ERRORS_CHECKSUM,
            };
            metric.write_meta(f)?;

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
                        S::State => DeviceStatusValue::from(&state).value(),
                        S::ErrorsRead => errors_read,
                        S::ErrorsWrite => errors_write,
                        S::ErrorsChecksum => errors_checksum,
                    };
                    context::Device {
                        pool_name,
                        dev_name: &dev_name,
                    }
                    .write_kv(f, metric, value.into())?;
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
