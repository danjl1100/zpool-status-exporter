# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A Prometheus-style exporter for ZFS pool status metrics, written in Rust. The application parses `zpool status` output to extract numeric metrics and expose them via an HTTP endpoint for monitoring systems.

## Architecture

- **Core Library** (`src/lib.rs`): Main application context, HTTP server implementation, and metrics generation
- **ZFS Module** (`src/zfs.rs`): Parses `zpool status` command output into structured data
- **Formatting Module** (`src/fmt/`): Converts parsed ZFS data to Prometheus metrics format
- **Authentication Module** (`src/auth.rs`): Basic HTTP authentication support
- **Binary Target** (`src/main.rs`): CLI entry point with argument parsing
- **Fake ZPool Binary** (`src/bin/fake-zpool.rs`): Test fixture that shadows real `zpool` command for integration tests

## Development Commands

### Core Development
- **Build**: `cargo build`
- **Run**: `cargo run -- 127.0.0.1:8976` (requires bind address)
- **Test**: `cargo test`

### Code Quality (Required before commits)
- **Linting**: `cargo clippy` - fix all warnings
- **Formatting**: `cargo fmt`
- **Documentation**: `cargo doc`

### Nix Development
- **Development Shell**: `nix develop` (includes cargo-expand, cargo-outdated, cargo-insta, alejandra)
- **Format Nix Files**: `find -iname '*.nix' -exec alejandra -q {} \;`
- **CI Tests**: `nix build .#ci` (runs all tests including VM tests)

### Integration Testing
- **End-to-End Tests**: Located in `tests/` directory with input/output fixtures
- **VM Tests**: `nix build .#vm-tests` (tests NixOS module integration)
- **Single Binary Test**: Run with `--oneshot-test-print` flag for metrics output

## Code Style & Standards

### Strict Safety Requirements
- **No unwrap**: `clippy::unwrap_used` is denied
- **No panic**: `clippy::panic` is denied
- **No unsafe**: `unsafe_code` is forbidden
- **Documentation required**: `missing_docs` is denied


### Error Handling
- Use `anyhow::Result` for application errors
- Structured error types with `std::error::Error` implementation
- Preserve error context through error chains

## Testing Strategy

### Unit Tests
- Parsing logic tests using snapshot testing (`insta` crate)
- Input fixtures in `tests/input/` with corresponding expected outputs
- Use `cargo insta review` to update snapshots

### Integration Tests
- `tests/single_integration_bin.rs`: End-to-end binary testing
- `tests/common/`: Shared test utilities for process execution and validation
- Fake `zpool` binary in test environment shadows real command

### VM Testing
- NixOS module integration testing
- Systemd service validation
- Network binding and authentication scenarios

## Configuration

### CLI Arguments
- `listen_address`: Network address to bind (required)
- `--basic-auth-keys-file`: Optional file containing authentication tokens

### Environment Variables
- Arguments can be set via environment variables (clap integration)

### NixOS Module
- Full NixOS module available at `nixosModules.default`
- Systemd service configuration with security hardening
- Configurable authentication and network binding

## HTTP Endpoints

- `/`: Root endpoint serving HTML status page
- `/metrics`: Prometheus-format metrics endpoint (requires auth if configured)

## Dependencies

### Core Runtime
- `tiny_http`: Lightweight HTTP server
- `clap`: CLI argument parsing with environment variable support
- `jiff`: Date/time handling and timezone support
- `tinytemplate`: HTML template rendering

### Development & Testing
- `insta`: Snapshot testing
- `tempfile`: Temporary file creation for tests

## Security Considerations

- Application refuses to run as root user
- Basic HTTP authentication support via file-based token lists
- Systemd service runs with restricted permissions
- No unsafe code allowed in codebase
