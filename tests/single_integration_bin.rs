//! Single integration test binary
//!
//! NOTE: Since the crate is primarily a "binary crate", the integration tests (running the
//! executable) are more important than library unit tests.
//!
//! As a general rule, there should only be one integration test binary, since integration tests
//! are run sequentially by cargo.
//!
//! Add as many `#[test]`s as you want! (in submodules of this `single_integration_bin`)

#![allow(clippy::panic)] // Tests can panic

mod common {
    const LISTEN_ADDRESS_END_TO_END: &str = "127.0.0.1:9582";
    const LISTEN_ADDRESS_END_TO_END_AUTH: &str = "127.0.0.1:9583";
    const LISTEN_ADDRESS_CHILD_STDERR_1: &str = "127.0.0.1:9584";
    const LISTEN_ADDRESS_CHILD_STDERR_2: &str = "127.0.0.1:9585";
    const LISTEN_ADDRESS_CHILD_SILENT_1: &str = "127.0.0.1:9586";
    const LISTEN_ADDRESS_CHILD_SILENT_2: &str = "127.0.0.1:9587";

    type MiniReqResult = Result<minreq::Response, minreq::Error>;

    mod child_silent;
    mod child_stderr;
    mod end_to_end;
    mod end_to_end_auth;

    mod sans_io_cases;

    mod bin_cmd;
}
const HTTP_OK: i32 = 200;
const HTTP_UNAUTHORIZED: i32 = 401;
const HTTP_FORBIDDEN: i32 = 403;
const HTTP_NOT_FOUND: i32 = 404;

// no HTTP_OK_STRING, as it varies by the actual response content
const HTTP_UNAUTHORIZED_STRING: &str = "Unauthorized";
const HTTP_FORBIDDEN_STRING: &str = "Forbidden";
const HTTP_NOT_FOUND_STRING: &str = "Not Found";

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

        assert!(response.len() >= expected.len(), "response too short for expected pattern\n\texpected = {expected:?}\n\tresponse = {response:?}");
        let (response_trimmed, response_remainder) = response.split_at(expected.len());

        // SANITY - verify ignored portion is numeric
        assert!(
            response_remainder.parse::<f64>().is_ok(),
            "non-numeric ignored remainder {response_remainder:?} of line {response:?}"
        );
        eprintln!("ignoring remainder {response_remainder:?} of line {response:?}");

        assert_eq!(response_trimmed, expected, "response_metrics line trimmed");
    } else {
        assert_eq!(response, expected, "response_metrics line");
    }
}

fn assert_response(
    label: &'static str,
    response: &minreq::Response,
    code: i32,
    check_fn: impl FnOnce(&str) -> bool,
) {
    let Ok(content) = response.as_str() else {
        panic!("expected UTF-8 response string for {label}");
    };

    assert_eq!(response.status_code, code, "{label} code");

    assert!(check_fn(content), "{label} check");
}
