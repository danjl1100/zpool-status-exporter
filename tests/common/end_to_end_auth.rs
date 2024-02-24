use super::MiniReqResult;
use crate::{
    assert_matches_template, assert_response,
    common::bin_cmd::{BinCommand, BinOutput},
    HTTP_FORBIDDEN, HTTP_FORBIDDEN_STRING, HTTP_NOT_FOUND, HTTP_NOT_FOUND_STRING, HTTP_OK,
    HTTP_UNAUTHORIZED, HTTP_UNAUTHORIZED_STRING,
};
use anyhow::Context as _;
use base64::Engine;
use std::io::Write as _;
use std::{net::SocketAddr, str::FromStr};

struct Responses {
    response_metrics_auth_none: MiniReqResult,
    response_metrics_auth_pass: MiniReqResult,
    response_metrics_auth_fail: MiniReqResult,
    response_root: MiniReqResult,
    response_unknown: MiniReqResult,
}

#[test]
fn run_bin() -> anyhow::Result<()> {
    const LISTEN_ADDRESS: &str = crate::common::LISTEN_ADDRESS_END_TO_END_AUTH;
    const EXPECTED_METRICS_OUTPUT: &str = include_str!("../../src/bin/output-integration.txt");

    let listen_address = SocketAddr::from_str(LISTEN_ADDRESS)?;

    let mut auth_file = tempfile::NamedTempFile::new().context("tempfile creation")?;
    writeln!(auth_file, "user1:word1")?;
    writeln!(auth_file, "user2:phrase2")?;
    let auth_file_name = auth_file
        .path()
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("non-displayable tempfile name"))?
        .to_string();

    // startup server
    let (output, responses) = BinCommand::new()
        .arg(LISTEN_ADDRESS)
        .arg("--basic-auth-keys-file")
        .arg_dynamic(auth_file_name)
        .spawn_cleanup_with(|| {
            // request from `/metrics` endpoint
            let response_metrics_auth_none =
                minreq::get(format!("http://{listen_address}/metrics")).send();

            // request from `/metrics` endpoint, expect PASS
            let userpass_b64 = base64::prelude::BASE64_STANDARD.encode("user1:word1");
            let response_metrics_auth_pass =
                minreq::get(format!("http://{listen_address}/metrics"))
                    .with_header("Authorization", format!("Basic {userpass_b64}"))
                    .send();

            // request from `/metrics` endpoint, expect FAIL
            // (s.i.c.  "phrase2" -> "phrase1" for fail case)
            let userfail_b64 = base64::prelude::BASE64_STANDARD.encode("user2:phrase1");
            let response_metrics_auth_fail =
                minreq::get(format!("http://{listen_address}/metrics"))
                    .with_header("Authorization", format!("Basic {userfail_b64}"))
                    .send();

            // request root `/`
            let response_root = minreq::get(format!("http://{listen_address}/")).send();

            // request non-existent URL
            let response_unknown = minreq::get(format!("http://{listen_address}/unknown"))
                .with_header("Authorization", format!("Basic {userpass_b64}"))
                .send();

            Responses {
                response_metrics_auth_none,
                response_metrics_auth_pass,
                response_metrics_auth_fail,
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
            format!(
                "Listening at http://{listen_address}\n{}",
                concat!(
                    "Allow-list configured with 2 entries\n",
                    "!!!!!! WARNING: HTTP transmits authentication in plaintext, use a HTTPS-proxy on the local machine!!!!!!!\n",
                    "denied access for \"user2:phrase1\" to url \"/metrics\"\n",
                )
            ),
            "stdout"
        );
        assert!(
            status.success(),
            "verify sleep duration after SIGINT, killing too early?"
        );
    }

    {
        let Responses {
            response_metrics_auth_none,
            response_metrics_auth_pass,
            response_metrics_auth_fail,
            response_root,
            response_unknown,
        } = responses;

        assert_response(
            "metrics_auth_none",
            response_metrics_auth_none?,
            HTTP_UNAUTHORIZED,
            |content| {
                assert_eq!(content, HTTP_UNAUTHORIZED_STRING, "metrics_auth_none");
                true
            },
        );

        assert_response(
            "metrics_auth_pass",
            response_metrics_auth_pass?,
            HTTP_OK,
            |content| {
                assert_matches_template(content, EXPECTED_METRICS_OUTPUT);
                true
            },
        );

        assert_response(
            "metrics_auth_fail",
            response_metrics_auth_fail?,
            HTTP_FORBIDDEN,
            |content| {
                assert_eq!(content, HTTP_FORBIDDEN_STRING, "metrics_auth_fail");
                true
            },
        );

        assert_response("root", response_root?, HTTP_OK, |content| {
            content.contains("zpool-status-exporter")
        });

        assert_response("unknown", response_unknown?, HTTP_NOT_FOUND, |content| {
            assert_eq!(content, HTTP_NOT_FOUND_STRING, "unknown");
            true
        });
    }

    Ok(())
}
