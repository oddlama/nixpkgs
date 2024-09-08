{ lib
, rustPlatform
, fetchFromGitHub
, pkg-config
, sqlite
, luajit
}:

rustPlatform.buildRustPackage {
  pname = "autokernel";
  version = "2.1.0";

  src = fetchFromGitHub {
    owner = "oddlama";
    repo = "autokernel";
    # rev = "v${version}";
    rev = "f9a5fdbb7356d31e9409e836987d9bda4f680834";
    hash = "sha256-6Lq3U4EAZHlYx07/HTEvWW1ETcvt5rMi74D+uHqvN4Q=";
  };

  cargoHash = "sha256-npn9LQJtx8sraFdbihBloNBtkXB7lp3nKb+95pzRoj8=";
  # Checks require running in a special environment where kernel build
  # scripts and tools are available
  doCheck = false;

  nativeBuildInputs = [
    pkg-config
  ];

  buildInputs = [
    sqlite
    luajit
  ];

  meta = with lib; {
    description = "A tool for managing your kernel configuration that guarantees semantic correctness";
    homepage = "https://github.com/oddlama/autokernel";
    license = licenses.mit;
    platforms = platforms.linux;
    maintainers = with maintainers; [ oddlama ];
    mainProgram = "autokernel";
  };
}
