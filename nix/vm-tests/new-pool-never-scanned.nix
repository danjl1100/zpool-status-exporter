{
  pkgs,
  nixosModule,
}: let
  listen_address = "127.0.0.1:1234";
  poolname = "newpool";
in
  pkgs.nixosTest {
    name = "new-pool-never-scanned";

    nodes.machine = {pkgs, ...}: {
      imports = [nixosModule];
      boot.supportedFilesystems = ["zfs"];
      networking.hostId = "abcd1234"; #arbitrary
      services.zpool-status-exporter = {
        enable = true;
        inherit listen_address;
      };
    };

    testScript = ''
      # Wait for system and service to be ready
      machine.wait_for_unit("default.target")
      machine.wait_for_unit("zpool-status-exporter.service")

      # Create file-backed disk images (64MB each)
      machine.succeed("dd if=/dev/zero of=/tmp/disk1.img bs=1M count=64")
      machine.succeed("dd if=/dev/zero of=/tmp/disk2.img bs=1M count=64")

      # Create mirror pool (new pool will have no scan line)
      machine.succeed("zpool create ${poolname} mirror /tmp/disk1.img /tmp/disk2.img")

      # Verify preconditions: pool is ONLINE and has never been scanned
      machine.succeed("zpool status ${poolname} | grep 'state: ONLINE'")
      machine.fail("zpool status ${poolname} | grep 'scan:'")

      # Validate metrics (wait_until_succeeds handles timing of pool detection)
      machine.wait_until_succeeds("curl http://${listen_address}/metrics | grep 'zpool_scan_state{pool=\"${poolname}\"} 40'")
      machine.wait_until_succeeds("curl http://${listen_address}/metrics | grep 'zpool_scan_age{pool=\"${poolname}\"} 876000'")
      machine.wait_until_succeeds("curl http://${listen_address}/metrics | grep 'NeverScanned = 40'")
    '';
  }
