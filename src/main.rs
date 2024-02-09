use clap::Parser;

fn main() -> anyhow::Result<()> {
    // SAFETY: no other threads exist in the process (first item in main)
    let time_context = unsafe { zpool_status_exporter::get_time_context() };

    if nix::unistd::Uid::effective().is_root() {
        anyhow::bail!("refusing to run as super-user, try a non-privileged user");
    }

    println!("Hello, world!");

    let args = zpool_status_exporter::Args::parse();
    time_context.serve(args)
}
