use clap::Parser;
use std::time::Instant;
use tiny_http::{Request, Response, Server};

#[derive(Parser)]
pub struct Args {
    #[clap(env)]
    pub listen_address: std::net::SocketAddr,
}

impl Args {
    pub fn serve(self) -> anyhow::Result<()> {
        let server = Server::http(self.listen_address).map_err(|e| anyhow::anyhow!(e))?;
        loop {
            let request = server.recv()?;
            let start_time = Instant::now();

            let _ = handle_request(request, start_time);
        }
    }
}
fn handle_request(request: Request, start_time: Instant) -> anyhow::Result<()> {
    const ENDPOINT_METRICS: &str = "/metrics";
    const HTML_NOT_FOUND: u32 = 404;
    let url = request.url();
    if url == ENDPOINT_METRICS {
        let response = get_metrics(start_time);
        Ok(request.respond(response)?)
    } else {
        let response = Response::empty(HTML_NOT_FOUND);
        Ok(request.respond(response)?)
    }
}

fn get_metrics(start_time: Instant) -> Response<impl std::io::Read> {
    let metrics = zfs::get_metrics();
    let metrics = fmt::format_metrics(metrics, start_time);
    Response::from_string(format!("hello there!, {metrics:?}"))
}

mod zfs {
    //! Parse the output of ZFS commands

    use std::process::Command;

    pub struct PoolMetrics {
        pub name: String,
        pub state: String,
        pub scan: String,
        pub devices: Vec<DeviceMetrics>,
    }
    pub struct DeviceMetrics {
        pub name: String,
        pub state: String,
        pub errors_read: u32,
        pub errors_write: u32,
        pub errors_checksum: u32,
    }

    pub fn get_metrics() -> Vec<PoolMetrics> {
        let output = match Command::new("zpool")
            .arg("status")
            .output()
            .map(|output| String::from_utf8(output.stdout))
        {
            Ok(Ok(output)) => output,
            Ok(Err(err)) => format!("{err}"),
            Err(err) => format!("{err}"),
        };
        // TODO
        // TODO
        // TODO
        vec![PoolMetrics {
            name: output,
            // TODO
            state: "".to_string(),
            scan: "".to_string(),
            devices: vec![],
        }]
    }
}

mod fmt {
    //! Organize metrics into the prometheus line-by-line format, with comments

    use crate::zfs::PoolMetrics;
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

    pub fn format_metrics(mut pools: Vec<PoolMetrics>, start_time: Instant) -> String {
        // TODO
        // TODO
        // TODO
        let duration = Instant::now().duration_since(start_time);
        pools
            .pop()
            .map(|PoolMetrics { name, .. }| format!("{name}, {duration:?}"))
            .unwrap()
    }
}
