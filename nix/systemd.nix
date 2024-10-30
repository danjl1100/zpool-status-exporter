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
in rec {
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

  render_service = {
    pkgs,
    name,
    service,
  }: let
    # NOTE: `fn` to exhaustively unpack the provided service attrs
    fn = {
      serviceConfig,
      description,
      wantedBy,
      path,
      environment,
    }:
      pkgs.symlinkJoin {
        name = "${name}_systemd_rendered";
        paths = let
          environment_attrs = {
            Environment =
              pkgs.lib.mapAttrsToList
              (name: value: "\"${name}=${value}\"")
              environment;
          };

          attrToLines = attrs:
            pkgs.lib.lists.flatten
            (pkgs.lib.mapAttrsToList (
                name: value:
                  if (builtins.isList value)
                  then
                    (builtins.map
                      (value: ''${name}=${toString value}'')
                      value)
                  else if (builtins.isBool value)
                  then ''${name}=${builtins.toJSON value}''
                  else [''${name}=${toString value}'']
              )
              attrs);

          lines =
            [
              ''
                [Unit]
                Description=${description}

                [Service]''
            ]
            ++ (attrToLines environment_attrs)
            ++ (attrToLines serviceConfig)
            ++ [
              ""
              "[Install]"
            ]
            ++ (attrToLines {WantedBy = wantedBy;});
        in [
          (pkgs.writeTextDir "${name}.service" (pkgs.lib.strings.concatLines lines))
        ];
      };
  in
    fn service;

  render_check = {
    nixpkgs,
    pkgs,
    zpool-status-exporter,
    nixosModules,
  }: let
    input_params = {
      listen_address = "127.0.0.1:4589739485";
      basic_auth_keys_file = "/path/to/secrets/basic_auth_keys_file.txt";
    };

    # use `pkgs` and `nixosModules` to build a system, to examine systemd output
    nixos-generated =
      (nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          nixosModules.default
          ({
            pkgs,
            modulesPath,
            ...
          }: {
            # FIXME - the resulting build isn't very minimal...
            # minimal
            imports = [(modulesPath + "/profiles/minimal.nix")];
            system.stateVersion = pkgs.lib.trivial.release;

            services.zpool-status-exporter = {
              package = zpool-status-exporter;
              enable = true;
              inherit
                (input_params)
                listen_address
                basic_auth_keys_file
                ;
            };
          })
        ];
      })
      .config
      .system
      .build
      .etc;

    # use `pkgs` and `zpool-status-exporter` to render the service manually
    rendered = let
      name = "zpool-status-exporter";
    in
      render_service {
        inherit
          pkgs
          name
          ;
        service = service {
          inherit
            zpool-status-exporter
            name
            ;
          zfs = "<PATH TO ZFS>";
          inherit
            (input_params)
            listen_address
            basic_auth_keys_file
            ;
        };
      };
  in
    pkgs.runCommand "check_systemd_render_ok" {
      UUT = "${rendered}";
      TRUTH = "${nixos-generated}/etc/systemd/system";
    } ''
      cd "$UUT"
      for f in *; do
        echo "Checking $f ..."
        echo diff -y "$TRUTH/$f" "$UUT/$f"
        diff -y <(grep -v 'Environment="LOCALE_ARCHIVE=' "$TRUTH/$f" | \
                grep -v 'Environment="PATH=' | \
                grep -v 'Environment="TZDIR=') \
            "$UUT/$f" || exit 1
      done
      mkdir $out
    '';
}
