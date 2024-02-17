{
  # NOTE: This `flake.nix` is just an entrypoint into `package.nix`
  #       Where possible, all metadata should be defined in `package.nix` for non-flake consumers
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
    arguments.parent_overlay = rust-overlay.overlays.default;
    arguments.for_package = {
      inherit
        advisory-db
        crane
        ;
      inherit (flake-utils.lib) mkApp;
    };
    nixos = import ./nix/nixos.nix {
      overlay = self.overlays.default;
    };
  in
    flake-utils.lib.eachSystem target_systems (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [arguments.parent_overlay];
        };

        package = pkgs.callPackage ./nix/package.nix arguments.for_package;

        alejandra = pkgs.callPackage ./nix/alejandra.nix {};
      in {
        inherit (package) apps;

        checks =
          package.checks
          // alejandra.checks;

        packages = let
          inherit (package) crate-name;
        in {
          ${crate-name} = package.${crate-name};
          default = package.${crate-name};

          vm-tests = pkgs.callPackage ./nix/vm-tests {
            inherit (nixos) nixosModules;
          };
        };

        devShells = {
          default = package.devShellFn {
            packages = [
              pkgs.alejandra
              pkgs.bacon
              pkgs.cargo-expand
            ];
          };
        };
      }
    )
    // {
      overlays.default = final: prev: let
        # apply parent overlay
        parent_overlay = arguments.parent_overlay final prev;

        package = final.callPackage ./nix/package.nix arguments.for_package;
      in
        parent_overlay
        // {
          # NOTE: infinite recursion when using `${crate-name} = ...` syntax
          inherit (package) zpool-status-exporter;
        };

      inherit (nixos) nixosModules;
    };
}
