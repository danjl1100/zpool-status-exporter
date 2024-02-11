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
    let input;
    let datetime = {
        const TEST_TIMESTAMP: &str = "TEST_TIMESTAMP=";
        let (timestamp_line, remainder) = full_input.split_once('\n').unwrap_or(("", full_input));
        input = remainder;

        let Some(timestamp_str) = timestamp_line.strip_prefix(TEST_TIMESTAMP) else {
            anyhow::bail!("missing timestamp line {TEST_TIMESTAMP:} in input")
        };

        time::OffsetDateTime::from_unix_timestamp(timestamp_str.parse()?)?
    };
    let compute_start_time = None; // compute time is unpredictable, cannot fake end duration

    zpool_status_exporter::TimeContext::new_assume_local_is_utc()
        .timestamp_at(datetime, compute_start_time)
        .get_metrics_for_output(input)
}

fn test_case(input: &str, expected: &str) -> anyhow::Result<()> {
    const SEPARATOR: &str = "------------------------------";
    let output = run_test(input)
        .with_context(|| format!("test case input:\n{SEPARATOR}\n{input}\n{SEPARATOR}"))?;
    assert_matches_template(&output, expected);
    Ok(())
}

#[test]
fn case1() -> anyhow::Result<()> {
    test_case(
        include_str!("../input/input-01-corrupted.txt"),
        include_str!("../input/output-01-corrupted.txt"),
    )
}

#[test]
fn case2() -> anyhow::Result<()> {
    test_case(
        include_str!("../input/input-02-online-data-corruption.txt"),
        include_str!("../input/output-02-online-data-corruption.txt"),
    )
}

// TODO
// #[test]
// fn case3() -> anyhow::Result<()> {
//     test_case(
//         include_str!("../input/input-03-resilvered.txt"),
//         include_str!("../input/output-03-resilvered.txt"),
//     )
// }
