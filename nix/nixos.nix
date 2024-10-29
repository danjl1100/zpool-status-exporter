{overlay}: {
  nixosModules.default = {
    config,
    lib,
    pkgs,
    ...
  }: let
    name = "zpool-status-exporter";
    cfg = config.services.${name};
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
      basic_auth_keys_file = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        description = ''
          Path to the file containing lines `user:pass` specifying allowed Basic authentication credentials
        '';
        default = null;
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
      systemd.services.${name} = (import ./systemd.nix).service {
        inherit name;
        inherit
          (cfg)
          listen_address
          basic_auth_keys_file
          ;
        zpool-status-exporter = cfg.package;
        zfs = config.boot.zfs.package;
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
