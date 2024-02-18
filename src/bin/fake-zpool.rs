//! Fake `zpool` stand-in for integration test usage

use clap::Parser as _;

const FAKE_INPUT: &str = include_str!("input-integration.txt");

#[derive(Default, clap::Parser)]
struct Args {
    arg0: String,

    #[clap(env)]
    #[arg(value_enum)]
    #[clap(default_value_t)]
    fake_zpool_mode: Mode,
}
#[derive(Clone, Copy, Default, clap::ValueEnum)]
enum Mode {
    #[default]
    Normal,
    NoPools,
    DevsMissing,
    Silent,
    SleepForever,
}

fn main() {
    let Args {
        arg0,
        fake_zpool_mode,
    } = Args::parse();

    match fake_zpool_mode {
        Mode::Normal => {
            if arg0 == "status" {
                // input for the parser = output by this `zpool` stand-in
                print!("{FAKE_INPUT}");
            } else {
                eprintln!("fake-zpool does not recognize argument {arg0:?}");
            }
        }
        Mode::NoPools => {
            eprintln!("no pools available");
        }
        Mode::DevsMissing => {
            eprintln!("/dev/zfs and /proc/self/mounts is needed, yada-yada...");
        }
        Mode::Silent => {}
        Mode::SleepForever => loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        },
    }
}
