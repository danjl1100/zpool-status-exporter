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

fn main() -> anyhow::Result<()> {
    let mut app_context = zpool_status_exporter::AppContext::new();
    {
        let cmd = <zpool_status_exporter::Args as clap::CommandFactory>::command();
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

    if nix::unistd::Uid::effective().is_root() {
        anyhow::bail!("refusing to run as super-user, try a non-privileged user");
    }

    if is_oneshot_test_print() {
        let metrics = app_context.get_metrics_now()?;
        println!("{metrics}");
        Ok(())
    } else {
        let args = zpool_status_exporter::Args::parse();
        app_context.serve(&args, Some(shutdown_rx))
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
