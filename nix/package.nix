{
  pkgs,
  crane,
  system,
  advisory-db,
  mkApp,
}: let
  # crate
  crate-name = "zpool-status-exporter";

  # no custom toolchain, that would be instead: (crane.mkLib pkgs).overrideToolchain rustToolchain
  craneLib = crane.mkLib pkgs;
  craneLibForDevShell = crane.mkLib pkgs;

  installOnlyBin = bin-name: "mkdir -p $out/bin; cp target/release/${bin-name} $out/bin/";

  crate = pkgs.callPackage ./crate.nix {
    inherit system advisory-db craneLib;
    src = let
      htmlFilter = path: _type: builtins.match ".*html$" path != null;
      txtFilter = path: _type: builtins.match ".*txt$" path != null;
      htmlOrTxtOrCargo = path: type:
        (htmlFilter path type) || (txtFilter path type) || (craneLib.filterCargoSources path type);
    in
      pkgs.lib.cleanSourceWith {
        src = craneLib.path ./..;
        filter = htmlOrTxtOrCargo;
      };
    extraBuildArgs = {
      installPhaseCommand = installOnlyBin crate-name;
    };
  };

  drv-open-doc = let
    open-cmd =
      if pkgs.stdenv.isDarwin
      then "open"
      else "${pkgs.xdg-utils}/bin/xdg-open";
    dash-to-underscores = input: builtins.replaceStrings ["-"] ["_"] input;
  in {
    for-crate = crate-name:
      pkgs.writeShellApplication {
        name = "open-doc-${crate-name}";
        text = ''
          echo "Opening docs for crate \"${crate-name}\""
          ${open-cmd} "file://${crate.doc}/share/doc/${dash-to-underscores crate-name}/index.html"
        '';
      };
    for-crate-deps = crate-name:
      pkgs.writeShellApplication {
        name = "open-doc-${crate-name}";
        text = ''
          echo "Opening docs for crate \"${crate-name}\""
          ${open-cmd} "file://${crate.doc-deps}/share/doc/${dash-to-underscores crate-name}/index.html"
        '';
      };
    inherit open-cmd;
  };

  app-default = mkApp {
    drv = crate.package;
  };
  apps-doc = {
    rust-doc = mkApp {
      drv = drv-open-doc.for-crate crate-name;
    };
    rust-doc-deps = mkApp {
      drv = drv-open-doc.for-crate-deps crate-name;
    };
  };
  apps =
    apps-doc
    // {
      default = app-default;
    };
in {
  inherit crate-name;
  ${crate-name} = crate.package;

  inherit
    (crate)
    checks
    tests-ignored
    ;

  inherit
    apps
    app-default
    apps-doc
    ;

  devShellFn = inputs:
    craneLibForDevShell.devShell (inputs
      // {
        inherit (crate) checks;
      });
}
