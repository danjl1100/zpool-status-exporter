use super::MiniReqResult;
use crate::{
    assert_matches_template,
    bin_cmd::{BinCommand, BinOutput},
    HTTP_NOT_FOUND, HTTP_OK,
};
use std::{net::SocketAddr, str::FromStr};

struct Responses {
    response_metrics: MiniReqResult,
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

            // request non-existent URL
            let response_unknown = minreq::get(format!("http://{listen_address}/")).send();

            Responses {
                response_metrics,
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
            response_unknown,
        } = responses;

        let response_metrics = response_metrics?;
        let response_metrics_status = response_metrics.status_code;
        let response_metrics = response_metrics.as_str()?;

        let response_unknown = response_unknown?;
        let response_unknown_status = response_unknown.status_code;
        let response_unknown = response_unknown.as_str()?;

        assert_eq!(response_unknown, "", "response_unknown");
        assert_eq!(response_unknown_status, HTTP_NOT_FOUND);

        assert_matches_template(response_metrics, EXPECTED_METRICS_OUTPUT);
        assert_eq!(response_metrics_status, HTTP_OK);
    }

    Ok(())
}
