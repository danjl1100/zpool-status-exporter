{
  pkgs,
  nixosModule,
}: let
  listen_address = "127.0.0.1:1234";
in
  pkgs.nixosTest {
    name = "local-services-test";
    nodes.machine = {pkgs, ...}: {
      imports = [nixosModule];
      boot.supportedFilesystems = ["zfs"];
      networking.hostId = "039419bd"; #arbitrary
      services.zpool-status-exporter = {
        enable = true;
        inherit listen_address;
      };
    };
    testScript = ''
      machine.wait_for_unit("default.target")
      machine.succeed("curl http://${listen_address}/metrics")
      machine.succeed("curl http://${listen_address}/metrics | grep '# no pools reported'")
      machine.succeed("curl http://${listen_address}/")
      machine.succeed("curl http://${listen_address}/ | grep 'zpool-status-exporter'")
    '';
  }
