//! In the spirit of "sans IO",
//! verify the INPUT string produces the correct OUTPUT string

use crate::assert_matches_template;
use anyhow::Context;

/// Compute the output string from the input string,
///
/// - `full_input` must contain a prepended line stating the "current datetime"
///   for the purpose of calculating duration metrics.
///
/// NOTE: The output does not include the total compute duration metric, to stay deterministic
///
fn run_test(full_input: &str) -> anyhow::Result<String> {
    let (input, timestamp) = {
        const TEST_TIMESTAMP: &str = "TEST_TIMESTAMP=";

        let (timestamp_line, remainder) = full_input.split_once('\n').unwrap_or(("", full_input));
        let input = remainder;

        let Some(timestamp_str) = timestamp_line.strip_prefix(TEST_TIMESTAMP) else {
            anyhow::bail!("missing timestamp line {TEST_TIMESTAMP:?} in input")
        };

        let timestamp = timestamp_str.parse()?;
        (input, timestamp)
    };
    let compute_start_time = None; // compute time is unpredictable, cannot fake end duration

    let metrics = zpool_status_exporter::AppContext::new_assume_local_is_utc()
        .timestamp_at_unix_utc(timestamp, compute_start_time)
        .ok_or_else(|| anyhow::anyhow!("invalid timestamp {timestamp} in input"))?
        .get_metrics_for_output(input)?;

    Ok(metrics)
}

fn test_case(input: &str, expected: &str) -> anyhow::Result<()> {
    const SEPARATOR: &str = "------------------------------";
    let output = run_test(input)
        .with_context(|| format!("test case input:\n{SEPARATOR}\n{input}\n{SEPARATOR}"))?;
    assert_matches_template(&output, expected);
    Ok(())
}

macro_rules! test_cases {
    (
        $(
            $test_label:ident {$($name:tt)+}
        )+
    ) => {
        $(
            #[test]
            fn $test_label() -> anyhow::Result<()> {
                test_case(
                    include_str!(concat!("../input/input-",  stringify!($($name)+), ".txt")),
                    include_str!(concat!("../input/output-", stringify!($($name)+), ".txt")),
                )
            }
        )+
    };
}

test_cases! {
    case01 {01-corrupted}
    case02 {02-online-data-corruption}
    case03 {03-resilvered}
    case04 {04-scrub-progress}
    case05 {05-features}
    case06 {06-removed}
    case07 {07-unavail}
    case08 {08-features-alt}
    case09 {09-scrub-cancel}
    case10 {10-new-pool}
    case11 {11-degraded-no-scan}
}
