{overlay}: {
  nixosModules.default = {
    config,
    lib,
    pkgs,
    ...
  }: let
    name = "zpool-status-exporter";
    cfg = config.services.${name};

    # a type for options that take a unit name
    # <https://github.com/NixOS/nixpkgs/blob/b971d88c583c796772ca9cea06e480d6d1980d73/nixos/lib/systemd-lib.nix#L64>
    unitNameType = lib.types.strMatching "[a-zA-Z0-9@%:_.\\-]+[.](service|socket|device|mount|automount|swap|target|path|timer|scope|slice)";
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
        default = "zpool-status-exporter";
      };
      group = lib.mkOption {
        type = lib.types.str;
        description = ''
          Group to run the zpool-status-exporter service
        '';
        default = "zpool-status-exporter";
      };
      basic_auth_keys_file = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        description = ''
          Path to the file containing lines `user:pass` specifying allowed Basic authentication credentials
        '';
        default = null;
      };
      wants = lib.mkOption {
        type = lib.types.listOf unitNameType;
        description = ''
          Start the specified units when this unit is started.
        '';
        default = ["network-online.target"];
      };
      after = lib.mkOption {
        type = lib.types.listOf unitNameType;
        description = ''
          If the specified units are started at the same time as this unit, delay this unit until they have started.
        '';
        default = ["network-online.target"];
      };
    };
    config = lib.mkIf cfg.enable {
      nixpkgs.overlays = [
        overlay
      ];
      users = lib.mkIf cfg.create_user_group {
        groups.${cfg.group} = {};
        users.${cfg.user} = {
          isSystemUser = true;
          description = "zpool-status-exporter server user";
          inherit (cfg) group;
        };
      };
      systemd.services.${name} = (import ./systemd.nix).service {
        inherit name;
        inherit
          (cfg)
          user
          group
          listen_address
          basic_auth_keys_file
          wants
          after
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
