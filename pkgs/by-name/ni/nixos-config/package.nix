{ rustPlatform }:
rustPlatform.buildRustPackage {
  pname = "nixos-config";
  version = "0.1.0";
  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
  meta.mainProgram = "nixos-config";
}
