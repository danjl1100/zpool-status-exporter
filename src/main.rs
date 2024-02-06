use clap::Parser;

fn main() -> anyhow::Result<()> {
    if nix::unistd::Uid::effective().is_root() {
        anyhow::bail!("refusing to run as super-user, try a non-privileged user");
    }

    println!("Hello, world!");

    zpool_status_exporter::Args::parse().serve()
}
