//! Binary for `zpool_status_exporter`

// teach me
#![deny(clippy::pedantic)]
// // no unsafe
// #![forbid(unsafe_code)]
// sane unsafe
#![forbid(unsafe_op_in_unsafe_fn)]
// no unwrap
#![deny(clippy::unwrap_used)]
// no panic
#![deny(clippy::panic)]
// docs!
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use clap::Parser as _;

/// Command-line arguments for the server
#[derive(clap::Parser)]
#[clap(version)]
struct Args {
    /// Bind address for the server
    #[clap(env)]
    listen_address: std::net::SocketAddr,
    /// Filename containing allowed basic authentication tokens
    #[clap(env)]
    #[arg(long)]
    basic_auth_keys_file: Option<std::path::PathBuf>,
}

fn main() -> anyhow::Result<()> {
    if nix::unistd::Uid::effective().is_root() {
        anyhow::bail!("refusing to run as super-user, try a non-privileged user");
    }

    let mut app_context = zpool_status_exporter::AppContext::new();
    {
        let cmd = <Args as clap::CommandFactory>::command();
        let app_version = cmd.get_version();
        app_context.set_app_version(app_version);
    }

    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        eprintln!("user requested shutdown...");
        shutdown_tx
            .send(zpool_status_exporter::Shutdown)
            .expect("termination channel send failed");
    })?;

    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if let Ok(zpool_status_exporter::Ready) = ready_rx.recv() {
            let notify_result = sd_notify::notify(true, &[sd_notify::NotifyState::Ready]);
            if let Err(err) = notify_result {
                eprintln!("error sending sd_notify Ready: {err}");
            }
        }
    });

    if is_oneshot_test_print() {
        let metrics = app_context.get_metrics_now()?;
        println!("{metrics}");
        Ok(())
    } else {
        let Args {
            listen_address,
            basic_auth_keys_file,
        } = Args::parse();
        let args =
            zpool_status_exporter::Args::listen_basic_auth(listen_address, basic_auth_keys_file);
        app_context
            .server_builder(&args)
            .set_ready_sender(ready_tx)
            .set_shutdown_receiver(shutdown_rx)
            .serve()?;
        Ok(())
    }
}

fn is_oneshot_test_print() -> bool {
    let mut args = std::env::args();
    if args.len() == 2 {
        let arg = args.nth(1).expect("second arg exists, in list of length 2");
        arg == "--oneshot-test-print"
    } else {
        false
    }
}
