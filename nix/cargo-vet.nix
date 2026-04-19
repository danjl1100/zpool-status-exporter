{
  stdenvNoCC,
  lib,
  cargo,
  cargo-vet,
  package,
  rustPlatform,
}: {
  checks = {
    cargo-vet-check = stdenvNoCC.mkDerivation {
      name = "cargo-vet-check";
      src = lib.cleanSourceWith {
        src = ./..;
        filter = path: type: let
          baseName = baseNameOf path;
          relativePath = lib.removePrefix (toString ./.. + "/") (toString path);
        in
          # Allow root Rust configuration files
          baseName
          == "Cargo.toml"
          || baseName == "Cargo.lock"
          # Allow directories and contents for cargo-vet inputs
          || baseName == "supply-chain"
          || lib.hasPrefix "supply-chain/" relativePath;
      };
      phases = ["unpackPhase" "buildPhase"];
      nativeBuildInputs = [
        cargo
        cargo-vet
        rustPlatform.cargoSetupHook # configures .cargo/config with vendored sources
      ];
      cargoDeps = package.cargoDeps;
      buildPhase = ''
        cargo vet check --locked --frozen
        touch $out
      '';
    };
  };
}
