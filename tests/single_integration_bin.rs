//! Single integration test binary
//!
//! NOTE: Since the crate is primarily a "binary crate", the integration tests (running the
//! executable) are more important than library unit tests.
//!
//! As a general rule, there should only be one integration test binary, since integration tests
//! are run sequentially by cargo.
//!
//! Add as many `#[test]`s as you want! (in submodules of this `single_integration_bin`)

mod common {
    mod end_to_end;

    // TODO
    // mod sans_io_cases;
}

/// line-by-line comparison, to filter out timestamp-sensitive items
fn assert_matches_template(response: &str, expected: &str) {
    const IGNORE_MARKER: &str = "<IGNORE>";

    println!("response:\n{response}\n--------------------------------------------------");
    println!("expected:\n{expected}\n--------------------------------------------------");

    let mut response = response.lines();
    let mut expected = expected.lines();
    loop {
        let response = response.next();
        let expected = expected.next();
        let (response, expected) = match (response, expected) {
            (None, None) => {
                break;
            }
            (Some(response), None) => {
                panic!("extra response line: {response:?}");
            }
            (None, Some(expected)) => {
                panic!("missing expected line: {expected:?}");
            }
            (Some(response), Some(expected)) => (response, expected),
        };
        assert_equals_ignore(response, expected, IGNORE_MARKER);
    }
}

fn assert_equals_ignore(response: &str, expected: &str, ignore: &str) {
    if expected.ends_with(ignore) {
        let (expected, after_ignore) = expected
            .split_once(ignore)
            .expect("contains marker because it also ends with marker");
        // SANITY - verify <IGNORE> is at end of line (e.g. only once in the line)
        assert_eq!(
            after_ignore, "",
            "only allowed one {ignore} per line, at end of line"
        );

        if response.len() < expected.len() {
            panic!("response too short for expected pattern\n\texpected = {expected:?}\n\tresponse = {response:?}");
        }
        let (response_trimmed, response_remainder) = response.split_at(expected.len());

        // SANITY - verify ignored portion is numeric
        if response_remainder.parse::<f64>().is_err() {
            panic!("non-numeric ignored remainder {response_remainder:?} of line {response:?}");
        };
        eprintln!("ignoring remainder {response_remainder:?} of line {response:?}");

        assert_eq!(response_trimmed, expected, "response_metrics line trimmed");
    } else {
        assert_eq!(response, expected, "response_metrics line");
    }
}
