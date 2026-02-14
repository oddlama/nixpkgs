# Test tracked config dependency tracking
{ lib, ... }:
let
  inherit (lib) mkOption types evalModules;

  # Separate evalModules calls to avoid thunk memoization cross-contamination
  mkResult = evalModules {
    modules = [
      ({ config, ... }: {
        options.base = mkOption { type = types.int; default = 1; };
        options.derived = mkOption { type = types.int; };
        options.combined = mkOption { type = types.int; };

        config.derived = config.base * 2;
        config.combined = config.base + config.derived;
      })
    ];
  };

  # For value correctness tests
  valResult = mkResult;

  # For dependency tests, use separate evaluation to avoid thunk interference
  depResult = evalModules {
    modules = [
      ({ config, ... }: {
        options.a = mkOption { type = types.int; default = 1; };
        options.b = mkOption { type = types.int; };
        config.b = config.a + 1;
      })
    ];
  };
in {
  # Value correctness
  test_base = valResult.config.base == 1;
  test_derived = valResult.config.derived == 2;
  test_combined = valResult.config.combined == 3;

  # Tracking availability
  test_makeTracked_available = builtins ? makeTracked;

  # Dependency tracking (queried before any other access to avoid thunk memoization)
  test_deps =
    if !(builtins ? makeTracked) then true
    else
      let deps = depResult.getConfigDependencies ["b"];
      in deps != null && builtins.length deps.dependencies > 0;
}
