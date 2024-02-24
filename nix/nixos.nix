{overlay}: {
  nixosModules.default = {
    config,
    lib,
    pkgs,
    ...
  }: let
    name = "zpool-status-exporter";
    cfg = config.services.${name};

    hardening = {
      # Hardening
      CapabilityBoundingSet = [""];
      DeviceAllow = [
        "/dev/zfs"
      ];
      LockPersonality = true;
      # PrivateDevices = true; # blocks all `DeviceAllow` devices
      PrivateUsers = true;
      ProcSubset = "pid";
      ProtectClock = true;
      ProtectControlGroups = true;
      ProtectHome = true;
      ProtectHostname = true;
      ProtectKernelLogs = true;
      ProtectKernelModules = true; # empirically not needed for ZFS kernel module access via zpool
      ProtectKernelTunables = true;
      ProtectProc = "invisible";
      RestrictAddressFamilies = ["AF_INET" "AF_INET6"];
      RestrictNamespaces = true;
      RestrictRealtime = true;
      SystemCallArchitectures = "native";
      SystemCallFilter = ["@system-service" "~@privileged" "~@resources"];
      UMask = "0077";
    };
  in {
    options.services.${name} = {
      enable = lib.mkEnableOption "${name} service";
      listen_address = lib.mkOption {
        type = lib.types.str;
        description = ''
          Socket address to listen for HTTP requests
        '';
        default = "127.0.0.1:8734";
      };
      package = lib.mkOption {
        type = lib.types.package;
        default = pkgs.zpool-status-exporter;
      };
      create_user_group = lib.mkOption {
        type = lib.types.bool;
        description = ''
          If `true`, creates the user and group for the service
        '';
        default = true;
      };
      user = lib.mkOption {
        type = lib.types.str;
        description = ''
          User to run the zpool-status-exporter service

          NOTE: Root is not allowed
        '';
      };
      group = lib.mkOption {
        type = lib.types.str;
        description = ''
          Group to run the zpool-status-exporter service
        '';
      };
    };
    config = lib.mkIf cfg.enable {
      nixpkgs.overlays = [
        overlay
      ];
      users = lib.mkIf cfg.create_user_group {
        groups.zpool-status-exporter = {};
        users.zpool-status-exporter = {
          isSystemUser = true;
          description = "zpool-status-exporter server user";
          group = "zpool-status-exporter";
        };
      };
      systemd.services.${name} = {
        description = "${name} server";
        # TODO remove Tmux-test serviceConfig
        # serviceConfig =
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
        serviceConfig =
          {
            Type = "simple";
            ExecStart = "${cfg.package}/bin/${name}";
            User = "zpool-status-exporter";
            Group = "zpool-status-exporter";
          }
          // hardening;
        wantedBy = ["default.target"];
        path = [config.boot.zfs.package];
        environment = {
          LISTEN_ADDRESS = cfg.listen_address;
        };
      };
      assertions = [
        {
          assertion = config.boot.zfs.enabled;
          message = ''
            The monitoring service `zpool-status-exporter` requires ZFS to be enabled.
            -> Try adding ZFS to `config.boot.supportedFilesystems` or `config.boot.initrd.supportedFilesystems`
          '';
        }
      ];
    };
  };
}
