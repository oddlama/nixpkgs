# Test dependency tracking in a real NixOS system evaluation.
#
# Usage:
#   nix eval --impure --json -f ./test-dep-tracking.nix | jq .
#
# This instantiates a minimal NixOS configuration and queries
# dependency information for system.build.toplevel and other options.

let
  nixpkgs = ./.;
  lib = import (nixpkgs + "/lib");

  # Evaluate a minimal NixOS configuration
  eval = import (nixpkgs + "/nixos/lib/eval-config.nix") {
    inherit lib;
    system = null;
    modules = [
      # Minimal required config
      ({ lib, ... }: {
        boot.loader.grub.enable = false;
        fileSystems."/".device = "/dev/sda1";
        nixpkgs.hostPlatform = "x86_64-linux";
      })
    ];
  };

  trackingAvailable = builtins ? makeTracked && builtins ? getAttrProvenance;

  # Collect dependencies for a list of option paths.
  # Each path is queried independently to avoid thunk memoization issues.
  queryDeps = paths:
    if !trackingAvailable then
      { error = "Tracking builtins not available (need custom nix with tracked attrset support)"; }
    else
      let
        queryOne = path:
          let
            deps = eval.getConfigDependencies path;
          in
          {
            name = lib.concatStringsSep "." path;
            value =
              if deps == null then
                { status = "no-provenance"; }
              else
                {
                  status = "ok";
                  inherit (deps) dependencies;
                };
          };
      in
      builtins.listToAttrs (map queryOne paths);

  # Top-level option paths to probe for dependencies
  interestingPaths = [
    [ "system" "build" "toplevel" ]
    [ "system" "build" "etc" ]
    [ "boot" "loader" "grub" "enable" ]
    [ "fileSystems" ]
    [ "networking" "hostName" ]
    [ "environment" "systemPackages" ]
    [ "services" "openssh" "enable" ]
    [ "security" "sudo" "enable" ]
  ];

in {
  inherit trackingAvailable;

  # Basic sanity: does the NixOS evaluation produce a valid config?
  sanity = {
    hasConfig = eval ? config;
    hasOptions = eval ? options;
    hasGetConfigDependencies = eval ? getConfigDependencies;
    hostName = eval.config.networking.hostName;
    config = eval.config;
  };

  # Dependency information for selected options
  dependencies = queryDeps interestingPaths;
}
