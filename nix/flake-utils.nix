# inlined from <https://github.com/numtide/flake-utils/blob/fa06cc1b3d9f8261138ab7e1bc54d115cfcdb6ea/lib.nix#L33> (MIT License)
let
  # Builds a map from <attr>=value to <attr>.<system>=value for each system.
  eachSystem = let
    # Applies a merge operation accross systems.
    eachSystemOp = op: systems: f:
      builtins.foldl' (op f) {} (
        if !builtins ? currentSystem || builtins.elem builtins.currentSystem systems
        then systems
        else
          # Add the current system if the --impure flag is used.
          systems ++ [builtins.currentSystem]
      );
  in
    eachSystemOp (
      # Merge outputs for each system.
      f: attrs: system: let
        ret = f system;
      in
        builtins.foldl' (
          attrs: key:
            attrs
            // {
              ${key} =
                (attrs.${key} or {})
                // {
                  ${system} = ret.${key};
                };
            }
        )
        attrs (builtins.attrNames ret)
    );

  # Returns the structure used by `nix app`
  mkApp = {
    drv,
    name ? drv.pname or drv.name,
    exePath ? drv.passthru.exePath or "/bin/${name}",
  }: {
    type = "app";
    program = "${drv}${exePath}";
  };

  lib = {
    inherit
      eachSystem
      mkApp
      ;
  };
in {
  inherit lib;
}
