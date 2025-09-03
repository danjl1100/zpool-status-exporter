{
  # NOTE: This `flake.nix` is just an entrypoint into `package.nix`
  #       Where possible, all metadata should be defined in `package.nix` for non-flake consumers
  description = "prometheus exporter for zpool-status metrics";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs";
  };

  outputs = {
    self,
    nixpkgs,
  }: let
    target_systems = [
      "x86_64-linux"
      # "aarch64-darwin"
    ];
    flake-utils = import ./nix/flake-utils.nix;
    nixos = import ./nix/nixos.nix {
      overlay = self.overlays.default;
    };

    systemd = import ./nix/systemd.nix;
  in
    flake-utils.lib.eachSystem target_systems (
      system: let
        pkgs = import nixpkgs {
          inherit system;
        };

        package = pkgs.callPackage ./default.nix {};

        alejandra = pkgs.callPackage ./nix/alejandra.nix {};

        systemd-render-check = systemd.render_check {
          inherit
            nixpkgs
            pkgs
            ;
          ${package.pname} = package;
          inherit (nixos) nixosModules;
        };
      in {
        checks =
          {inherit package;}
          // alejandra.checks;

        packages = let
          vm-tests = pkgs.callPackage ./nix/vm-tests {
            inherit (nixos) nixosModules;
          };

          all-long-tests = pkgs.symlinkJoin {
            name = "all-long-tests";
            paths = [
              vm-tests
              systemd-render-check
            ];
          };
        in {
          ${package.pname} = package;
          default = package;

          inherit
            all-long-tests
            vm-tests
            systemd-render-check
            ;

          # alias for convenience
          ci = all-long-tests;
        };

        devShells = {
          default = pkgs.mkShell {
            nativeBuildInputs = [
              pkgs.alejandra
              pkgs.bacon
              pkgs.cargo-expand
              pkgs.cargo-outdated
              pkgs.cargo-insta
            ];
          };
        };
      }
    )
    // {
      overlays.default = final: prev: {
        zpool-status-exporter = final.callPackage ./default.nix {};
      };

      inherit (nixos) nixosModules;

      lib = {
        systemd = {
          inherit (systemd) service render_service;
        };
      };
    };
}
