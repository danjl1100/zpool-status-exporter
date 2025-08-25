{
  pkgs,
  nixosModule,
}: let
  listen_address = "127.0.0.1:1234";
in
  pkgs.nixosTest {
    name = "max-bind-retries";
    nodes.machine = {pkgs, ...}: {
      imports = [nixosModule];
      boot.supportedFilesystems = ["zfs"];
      networking.hostId = "239419bd"; #arbitrary

      services.zpool-status-exporter = {
        enable = true;
        inherit listen_address;
        maxBindRetries = 10; # Test with more retries to ensure it works
      };
    };
    testScript = ''
      # Start the machine
      machine.start()

      # Wait for the service to start successfully
      machine.wait_for_unit("zpool-status-exporter.service")

      # Verify the service is running and responding
      machine.succeed("curl http://${listen_address}/metrics")
      machine.succeed("curl http://${listen_address}/metrics | grep '# no pools reported'")
      machine.succeed("curl http://${listen_address}/")
      machine.succeed("curl http://${listen_address}/ | grep 'zpool-status-exporter'")

      # Verify the MAX_BIND_RETRIES environment variable was set correctly
      machine.succeed("systemctl show zpool-status-exporter.service | grep 'Environment.*MAX_BIND_RETRIES=10'")
    '';
  }
