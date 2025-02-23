{
  # NOTE: This `flake.nix` is just an entrypoint into `package.nix`
  #       Where possible, all metadata should be defined in `package.nix` for non-flake consumers
  description = "prometheus exporter for zpool-status metrics";

  inputs = {
    crane.url = "github:ipetkov/crane";
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-24.11";
    flake-compat.url = "github:nix-community/flake-compat";
  };

  outputs = {
    # common
    self,
    nixpkgs,
    flake-compat,
    # rust
    crane,
    advisory-db,
  }: let
    target_systems = [
      "x86_64-linux"
      # "aarch64-darwin"
    ];
    arguments.for_package = {
      inherit
        advisory-db
        crane
        ;
      inherit mkApp;
    };
    nixos = import ./nix/nixos.nix {
      overlay = self.overlays.default;
    };

    # inlined from <https://github.com/numtide/flake-utils/blob/fa06cc1b3d9f8261138ab7e1bc54d115cfcdb6ea/lib.nix#L33>
    # Builds a map from <attr>=value to <attr>.<system>=value for each system.
    eachSystem = let
      # Applies a merge operation accross systems.
      eachSystemOp = op: systems: f:
        builtins.foldl' (op f) {} (
          if !builtins ? currentSystem || builtins.elem builtins.currentSystem systems
          then systems
          else
            # Add the current system if the --impure flag is used.
            systems ++ [builtins.currentSystem]
        );
    in
      eachSystemOp (
        # Merge outputs for each system.
        f: attrs: system: let
          ret = f system;
        in
          builtins.foldl' (
            attrs: key:
              attrs
              // {
                ${key} =
                  (attrs.${key} or {})
                  // {
                    ${system} = ret.${key};
                  };
              }
          )
          attrs (builtins.attrNames ret)
      );

    # inlined from <https://github.com/numtide/flake-utils/blob/fa06cc1b3d9f8261138ab7e1bc54d115cfcdb6ea/lib.nix#L33>
    # Returns the structure used by `nix app`
    mkApp = {
      drv,
      name ? drv.pname or drv.name,
      exePath ? drv.passthru.exePath or "/bin/${name}",
    }: {
      type = "app";
      program = "${drv}${exePath}";
    };

    systemd = import ./nix/systemd.nix;
  in
    eachSystem target_systems (
      system: let
        pkgs = import nixpkgs {
          inherit system;
        };

        package = pkgs.callPackage ./nix/package.nix arguments.for_package;

        alejandra = pkgs.callPackage ./nix/alejandra.nix {};

        systemd-render-check = systemd.render_check {
          inherit
            nixpkgs
            pkgs
            ;
          zpool-status-exporter = package.${package.crate-name};
          inherit (nixos) nixosModules;
        };
      in {
        inherit (package) apps;

        checks =
          package.checks
          // alejandra.checks;

        packages = let
          inherit (package) crate-name;

          vm-tests = pkgs.callPackage ./nix/vm-tests {
            inherit (nixos) nixosModules;
          };

          all-long-tests = pkgs.symlinkJoin {
            name = "all-long-tests";
            paths = [
              vm-tests
              package.tests-ignored
              systemd-render-check
            ];
          };
        in {
          ${crate-name} = package.${crate-name};
          default = package.${crate-name};

          inherit
            all-long-tests
            vm-tests
            systemd-render-check
            ;

          # alias for convenience
          ci = all-long-tests;
        };

        devShells = {
          default = package.devShellFn {
            packages = [
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
      overlays.default = final: prev: let
        package = final.callPackage ./nix/package.nix arguments.for_package;
      in {
        # NOTE: infinite recursion when using `${crate-name} = ...` syntax
        inherit (package) zpool-status-exporter;
      };

      inherit (nixos) nixosModules;

      lib = {
        systemd = {
          inherit (systemd) service render_service;
        };
      };
    };
}
