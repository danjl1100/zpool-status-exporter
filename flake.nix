{
  description = "prometheus exporter for zpool-status metrics";

  inputs = {
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
    # decrease total count of flake dependencies by following versions from "rust-overlay" input
    flake-utils.follows = "rust-overlay/flake-utils";
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-23.11";
    crane.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = {
    # common
    self,
    flake-utils,
    nixpkgs,
    # rust
    rust-overlay,
    crane,
    advisory-db,
  }: let
    target_systems = [
      "x86_64-linux"
      # "aarch64-darwin"
    ];
    nixos = import ./nix/nixos.nix {
      inherit (self) packages;
    };
  in
    flake-utils.lib.eachSystem target_systems (
      system: let
        overlays = [
          rust-overlay.overlays.default
        ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # crate
        crate-name = "zpool-status-exporter";

        rustChannel = "beta";
        rustVersion = "latest";
        rustToolchain = pkgs.rust-bin.${rustChannel}.${rustVersion}.default;
        rustToolchainForDevshell = rustToolchain.override {
          extensions = ["rust-analyzer" "rust-src"];
        };
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
        craneLibForDevShell = (crane.mkLib pkgs).overrideToolchain rustToolchainForDevshell;

        crate = pkgs.callPackage ./nix/crate.nix {
          inherit system advisory-db craneLib;
          src = craneLib.path ./.;
          extraBuildArgs = {
            installPhaseCommand = "mkdir -p $out/bin; cp target/release/${crate-name} $out/bin/";
          };
        };
      in rec {
        checks =
          crate.checks
          // {
            nix-alejandra = pkgs.stdenvNoCC.mkDerivation {
              name = "nix-alejandra";
              src = pkgs.lib.cleanSourceWith {
                src = ./.;
                filter = path: type: ((type == "directory") || (pkgs.lib.hasSuffix ".nix" path));
              };
              phases = ["buildPhase"];
              nativeBuildInputs = [pkgs.alejandra];
              buildPhase = "(alejandra -qc $src || alejandra -c $src) > $out";
            };
          };

        packages = {
          default = crate.package;
          ${crate-name} = crate.package;
        };

        apps = {
          rust-doc = flake-utils.lib.mkApp {
            drv = crate.drv-open-doc.for-crate crate-name;
          };
          rust-doc-deps = flake-utils.lib.mkApp {
            drv = crate.drv-open-doc.for-crate-deps crate-name;
          };
          rust-doc-std = flake-utils.lib.mkApp {
            drv = crate.drv-open-doc.for-std rustToolchainForDevshell;
          };
        };

        devShells = {
          default = crate.devShellFn {
            craneLib = craneLibForDevShell;
            packages = [
              pkgs.alejandra
              pkgs.bacon
              pkgs.cargo-expand
            ];
          };
        };
      }
    );
}
