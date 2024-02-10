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
    pub enum ScanStatusValue for ScanStatus {
        #[default]
        UnknownMissing => 0,
        Unrecognized => 1,
        // healthy
        ScrubRepaired => 10,
        // errors
        // TODO Add new statuses here
    }
    pub enum ErrorStatusValue for ErrorStatus {
        #[default]
        UnknownMissing => 0,
        Unrecognized => 1,
        // healthy
        Ok => 10,
        // errors
        // TODO Add new errors here
    }
}

use crate::zfs::{DeviceStatus, ErrorStatus, PoolMetrics, ScanStatus};
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
                    writeln!(f, "# Pool state: {}", DeviceStatusValue::summarize_values())?;
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
                    Sections::PoolState => DeviceStatusValue::from_opt(state).into(),
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
                writeln!(f, "{PREFIX}_{metric}{{pool={name:?}}}={value:.precision$}")?;
            }
        }

        writeln!(f, "# total duration of the lookup in microseconds")?;
        let lookup_duration_micros = start_time.elapsed().as_micros();
        writeln!(f, "{PREFIX}_lookup={lookup_duration_micros}")
    }
}
