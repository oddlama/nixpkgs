{
  rustPlatform,
}:
rustPlatform.buildRustPackage {
  pname = "json2nix";
  version = "0.1.0";
  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
}
