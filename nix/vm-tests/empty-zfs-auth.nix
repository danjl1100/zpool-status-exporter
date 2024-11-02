{
  pkgs,
  nixosModule,
}: let
  listen_address = "127.0.0.1:1234";
in
  pkgs.nixosTest {
    name = "empty-zfs-auth";
    nodes.machine = {pkgs, ...}: {
      imports = [nixosModule];
      boot.supportedFilesystems = ["zfs"];
      networking.hostId = "139419bd"; #arbitrary
      services.zpool-status-exporter = {
        enable = true;
        inherit listen_address;
        # NOTE: in real-world use case, don't put authentication info in the *PUBLIC* /nix/store
        basic_auth_keys_file = pkgs.writeText "auth-keys-file" ''
          john:doe
          alice:bob
        '';
      };
    };
    testScript = ''
      machine.wait_for_unit("default.target")
      machine.wait_for_unit("zpool-status-exporter.service")
      machine.fail("curl --fail http://${listen_address}/metrics")
      machine.succeed("curl --fail -u john:doe http://${listen_address}/metrics")
      machine.succeed("curl --fail -u john:doe http://${listen_address}/metrics | grep '# no pools reported'")
      machine.succeed("curl --fail -u alice:bob http://${listen_address}/metrics")
      machine.fail("curl --fail -u john:bob http://${listen_address}/metrics")
      machine.succeed("curl --fail http://${listen_address}/")
      machine.succeed("curl --fail http://${listen_address}/ | grep 'zpool-status-exporter'")
    '';
  }
