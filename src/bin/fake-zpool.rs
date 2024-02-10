//! Fake `zpool` stand-in for integration test usage

const FAKE_INPUT: &str = include_str!("input-integration.txt");

fn main() {
    // input for the parser = output by this `zpool` stand-in
    print!("{FAKE_INPUT}");
}
