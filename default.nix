{
  pkgs ? import <nixpkgs> {},
  lib ? pkgs.lib,
  rustPlatform ? pkgs.rustPlatform,
}:
rustPlatform.buildRustPackage rec {
  pname = "zpool-status-exporter";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type: let
      baseName = baseNameOf path;
      relativePath = lib.removePrefix (toString ./. + "/") (toString path);
    in
      # Allow root Rust files and configuration
      baseName
      == "Cargo.toml"
      || baseName == "Cargo.lock"
      || baseName == "default.nix"
      # Allow directories and contents for Rust source
      || baseName == "src"
      || baseName == "tests"
      || lib.hasPrefix "src/" relativePath
      || lib.hasPrefix "tests/" relativePath;
  };

  cargoLock = {
    lockFile = ./Cargo.lock;
  };
}
