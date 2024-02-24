use super::MiniReqResult;
use crate::{
    assert_matches_template, assert_response,
    common::bin_cmd::{BinCommand, BinOutput},
    HTTP_NOT_FOUND, HTTP_NOT_FOUND_STRING, HTTP_OK,
};
use std::{net::SocketAddr, str::FromStr};

struct Responses {
    response_metrics: MiniReqResult,
    response_root: MiniReqResult,
    response_unknown: MiniReqResult,
}

#[test]
fn run_bin() -> anyhow::Result<()> {
    const LISTEN_ADDRESS: &str = crate::common::LISTEN_ADDRESS_END_TO_END;
    const EXPECTED_METRICS_OUTPUT: &str = include_str!("../../src/bin/output-integration.txt");

    let listen_address = SocketAddr::from_str(LISTEN_ADDRESS)?;

    // startup server
    let (output, responses) = BinCommand::new()
        .arg(LISTEN_ADDRESS)
        .spawn_cleanup_with(|| {
            // request from `/metrics` endpoint
            let response_metrics = minreq::get(format!("http://{listen_address}/metrics")).send();

            // request root `/`
            let response_root = minreq::get(format!("http://{listen_address}/")).send();

            // request non-existent URL
            let response_unknown = minreq::get(format!("http://{listen_address}/unknown")).send();

            Responses {
                response_metrics,
                response_root,
                response_unknown,
            }
        })?;

    {
        let BinOutput {
            status,
            stdout,
            stderr,
        } = output;

        // no fatal errors
        //
        // "NOTSURE?" is mentioned twice:
        // 1. once for fail-fast startup run, and
        // 2. again for the "/metrics" request
        assert_eq!(
            stderr,
            concat!(
                "Unrecognized DeviceStatus: \"NOTSURE?\"\n",
                "Unrecognized DeviceStatus: \"NOTSURE?\"\n",
                "user requested shutdown...\n",
            ),
            "stderr"
        );
        assert_eq!(
            stdout,
            format!("Listening at http://{listen_address}\n"),
            "stdout"
        );
        assert!(
            status.success(),
            "verify sleep duration after SIGINT, killing too early?"
        );
    }

    {
        let Responses {
            response_metrics,
            response_root,
            response_unknown,
        } = responses;

        assert_response("root", response_root?, HTTP_OK, |content| {
            content.contains("zpool-status-exporter")
        });

        assert_response("unknown", response_unknown?, HTTP_NOT_FOUND, |content| {
            assert_eq!(content, HTTP_NOT_FOUND_STRING, "unknown");
            true
        });

        assert_response("metrics", response_metrics?, HTTP_OK, |content| {
            assert_matches_template(content, EXPECTED_METRICS_OUTPUT);
            true
        });
    }

    Ok(())
}
