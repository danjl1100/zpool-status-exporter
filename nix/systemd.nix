let
  hardening = {
    # Hardening
    CapabilityBoundingSet = [""];
    DeviceAllow = [
      "/dev/zfs"
    ];
    LockPersonality = true;
    # PrivateDevices = true; # blocks all `DeviceAllow` devices
    # PrivateUsers = true; # blocks some capability needed for zpool to locate pools
    ProcSubset = "pid";
    ProtectClock = true;
    ProtectControlGroups = true;
    ProtectHome = true;
    ProtectHostname = true;
    ProtectKernelLogs = true;
    # ProtectKernelModules = true; # need ZFS kernel module access for zpool command
    ProtectKernelTunables = true;
    ProtectProc = "invisible";
    RestrictAddressFamilies = ["AF_INET" "AF_INET6"];
    RestrictNamespaces = true;
    RestrictRealtime = true;
    SystemCallArchitectures = "native";
    SystemCallFilter = ["@system-service" "~@privileged" "~@resources"];
    UMask = "0077";
  };
  # TODO remove Tmux-test serviceConfig
  # tmuxTestServiceConfig = pkgs:
  #   {
  #     # enter via:   tmux -S /run/myService/tmux.socket attach
  #     ExecStart = "${pkgs.tmux}/bin/tmux -S /run/myService/tmux.socket new-session -s my-session -d";
  #     ExecStop = "${pkgs.tmux}/bin/tmux -S /run/myService/tmux.socket kill-session -t my-session";
  #     Type = "forking";
  #     # Used as root directory
  #     RuntimeDirectory = "myService";
  #     RootDirectory = "/run/myService";
  #     BindReadOnlyPaths = [
  #       "/nix/store"
  #       # So tmux uses /bin/sh as shell
  #       "/bin"
  #     ];
  #     # This sets up a private /dev/tty
  #     # The tmux server would crash without this
  #     # since there would be nothing in /dev
  #     # PrivateDevices = true;
  #   }
  #   // hardening;
in {
  service = {
    zpool-status-exporter,
    zfs,
    name,
    listen_address,
    basic_auth_keys_file,
  }: {
    description = "${name} server";
    serviceConfig =
      {
        Type = "simple";
        ExecStart = "${zpool-status-exporter}/bin/zpool-status-exporter";
        User = "zpool-status-exporter";
        Group = "zpool-status-exporter";
      }
      // hardening;
    wantedBy = ["default.target"];
    path = [zfs];
    environment = {
      LISTEN_ADDRESS = listen_address;
      BASIC_AUTH_KEYS_FILE = basic_auth_keys_file;
    };
  };
}
