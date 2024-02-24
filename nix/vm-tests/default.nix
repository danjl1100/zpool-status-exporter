{
  callPackage,
  symlinkJoin,
  nixosModules,
}: let
  test_sources = {
    empty-zfs = ./empty-zfs.nix;
    empty-zfs-auth = ./empty-zfs-auth.nix;
  };
  tests = builtins.mapAttrs (_name: test_source:
    callPackage test_source {
      nixosModule = nixosModules.default;
    })
  test_sources;
in
  symlinkJoin {
    name = "vm-tests";
    paths = builtins.attrValues tests;
  }
  // {
    # allow building vm-tests.tests.<test-name>.buildInteractive
    # NOTE: run interactively via
    #           nix build .#vm-tests.tests.<<TEST_NAME>>.driverInteractive && ./result/bin/nixos-test-driver
    inherit tests;
  }
