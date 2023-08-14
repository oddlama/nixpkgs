{ buildGoModule
, fetchFromGitHub
, lib
}:

let
  version = "1.0.0";

  src = fetchFromGitHub {
    owner = "oddlama";
    repo = "influxdb2-token-manipulator";
    rev = "9d3e1bd00ba1d8caa10fa9af990089d7a81b761c"; #"v${version}";
    hash = "sha256-CScERPVyLRdLPNvBeTzZNc8uJxLLJYsdhdg0nbN7KBU=";
  };

in buildGoModule {
  pname = "influxdb2-token-manipulator";
  inherit version src;

  vendorHash = "sha256-zBZk7JbNILX18g9+2ukiESnFtnIVWhdN/J/MBhIITh8=";

  meta = with lib; {
    description = "Utility program to manipulate influxdb api tokens for declarative setups";
    homepage = "https://github.com/oddlama/influxdb2-token-manipulator";
    license = licenses.mit;
    maintainers = with maintainers; [oddlama];
    mainProgram = "influxdb2-token-manipulator";
  };
}
