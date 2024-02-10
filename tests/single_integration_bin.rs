//! Single integration test binary
//!
//! NOTE: Since the crate is primarily a "binary crate", the integration tests (running the
//! executable) are more important than library unit tests.
//!
//! As a general rule, there should only be one integration test binary, since integration tests
//! are run sequentially by cargo.
//!
//! Add as many `#[test]`s as you want! (in submodules of this `single_integration_bin`)

mod common {
    mod end_to_end;
}
