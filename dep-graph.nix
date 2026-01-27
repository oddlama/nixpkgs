# Generate a full dependency graph for a NixOS evaluation.
#
# Usage:
#   ./build/src/nix/nix eval --impure --json -f ./nixpkgs/dep-graph.nix depsOnly | jq .
#   ./build/src/nix/nix eval --impure --raw -f ./nixpkgs/dep-graph.nix d2Filtered > graph.d2
#   d2 graph.d2 graph.svg
#
# Starting from system.build.toplevel, recursively discovers all option
# dependencies and their values, producing a complete graph of every option
# that influenced the final system configuration.
#
# The graph is a nested attrset mirroring the NixOS option namespace.
# Each node stores its data under a __node key to avoid collisions:
#
#   graph.system.build.toplevel.__node.deps = [ ["boot" "kernelParams"] ... ];
#   graph.system.build.toplevel.__node.value = <derivation>;

let
  nixpkgs = ./.;
  lib = import (nixpkgs + "/lib");

  # --- NixOS evaluation ---------------------------------------------------

  eval = import (nixpkgs + "/nixos/lib/eval-config.nix") {
    inherit lib;
    system = null;
    modules = [
      ({ lib, ... }: {
        boot.loader.grub.enable = false;
        fileSystems."/".device = "/dev/sda1";
        nixpkgs.hostPlatform = "x86_64-linux";
      })
    ];
  };

  # --- helpers -------------------------------------------------------------

  inherit (lib)
    attrByPath concatStringsSep concatMap filter foldl' init isAttrs last
    length listToAttrs sort;

  # Canonical key for a path list — used only internally for dedup/visited tracking.
  # builtins.toJSON is unambiguous for any list of strings regardless of content.
  pathKey = builtins.toJSON;

  # --- Nix-style path rendering --------------------------------------------
  #
  # Renders a path like ["fileSystems" "/" "device"] as: fileSystems."/".device
  # Segments that are valid Nix identifiers are bare; others are quoted.

  isValidIdent = s: builtins.match "[a-zA-Z_][a-zA-Z0-9_'-]*" s != null;

  escapeNixStr = s:
    lib.replaceStrings [ ''"'' "\\" "$" ] [ ''\"'' "\\\\" "\\$" ] s;

  quoteSegment = s:
    if isValidIdent s then s
    else ''"'' + escapeNixStr s + ''"'';

  nixPath = path: concatStringsSep "." (map quoteSegment path);

  # --- dependency query via getConfigDependencies --------------------------

  # Returns a list of dependency paths (each a list of strings), or [].
  queryDeps = path:
    let result = eval.getConfigDependencies path;
    in if result == null then [] else result.dependencies;

  # Get the value of an option path from the (real) config.
  getValue = path: attrByPath path null eval.config;

  # --- leaf filtering ------------------------------------------------------

  # Remove any path that is a strict prefix of another.
  isPrefix = a: b:
    (length a < length b) && (lib.take (length a) b == a);

  removeParents = paths:
    let sorted = sort (a: b: nixPath a < nixPath b) paths;
    in filter (p: !(lib.any (other: isPrefix p other) sorted)) sorted;

  # Deduplicate paths using their canonical key.
  dedupPaths = paths:
    lib.attrValues (listToAttrs (map (p: { name = pathKey p; value = p; }) paths));

  # --- fixed-point dependency crawl ----------------------------------------
  #
  # Internal flat graph: keyed by pathKey, values are
  #   { path = [...]; deps = [[...] ...]; value = ...; }
  # Converted to nested attrsets only at the output boundary.

  step = graph:
    let
      allDepPaths = foldl' (acc: key:
        acc ++ graph.${key}.deps
      ) [] (builtins.attrNames graph);

      newPaths = filter (p: !(graph ? ${pathKey p})) (dedupPaths allDepPaths);

      newEntries = map (path:
        let
          rawDeps = queryDeps path;
          leafDeps = removeParents (dedupPaths rawDeps);
        in {
          name = pathKey path;
          value = { inherit path; deps = leafDeps; value = getValue path; };
        }
      ) newPaths;
    in
    graph // (listToAttrs newEntries);

  # Iterate until stable (cap at 50 to avoid runaways).
  crawl = graph0:
    let
      go = n: graph:
        if n <= 0 then graph
        else
          let next = step graph;
          in
          if builtins.attrNames next == builtins.attrNames graph
          then graph
          else go (n - 1) next;
    in
    go 50 graph0;

  # --- entry point ---------------------------------------------------------

  rootPath = [ "system" "build" "toplevel" ];

  rootDepsRaw = queryDeps rootPath;
  rootDeps = removeParents (dedupPaths rootDepsRaw);

  seed = {
    ${pathKey rootPath} = {
      path = rootPath;
      deps = rootDeps;
      value = getValue rootPath;
    };
  };

  flatGraph = crawl seed;
  flatEntries = map (k: flatGraph.${k}) (builtins.attrNames flatGraph);

  # --- output: nested attrsets ---------------------------------------------
  #
  # Convert flat graph to nested attrset.  Each node is stored under __node
  # to avoid collisions with child paths (e.g., both "boot.zfs" and
  # "boot.zfs.enabled" can coexist).

  toNested = mkNode:
    foldl' lib.recursiveUpdate {} (
      map (e: lib.setAttrByPath (e.path ++ [ "__node" ]) (mkNode e)) flatEntries
    );

  # --- d2lang output -------------------------------------------------------
  #
  # Usage:
  #   ./build/src/nix/nix eval --impure --raw -f ./nixpkgs/dep-graph.nix d2 > graph.d2
  #   d2 graph.d2 graph.svg
  #
  # Node IDs use the Nix-style quoted path inside d2 double-quotes, so d2
  # treats the whole thing as a single identifier (no container nesting).
  # Example:  "fileSystems.\"/\".device" -> "boot.loader.grub.enable"

  d2Escape = s: lib.replaceStrings [ ''"'' "\\" ] [ ''\"'' "\\\\" ] s;

  d2NodeId = path: ''"'' + d2Escape (nixPath path) + ''"'';

  mkD2 = entries:
    let
      nodeDecls = map (e: d2NodeId e.path) entries;

      edges = concatMap (e:
        map (dep: "${d2NodeId e.path} -> ${d2NodeId dep}") e.deps
      ) entries;
    in
    concatStringsSep "\n" (
      [ "# NixOS dependency graph — ${toString (length entries)} nodes"
        "# Generated by dep-graph.nix"
        "direction: right"
        ""
      ]
      ++ nodeDecls
      ++ [ "" ]
      ++ edges
    ) + "\n";

in
  if !(builtins ? makeTracked && builtins ? getAttrProvenance) then
    { error = "Tracking builtins not available (need custom nix with tracked attrset support)"; }
  else rec {
    nodeCount = length flatEntries;
    root = rootPath;

    # The full graph with values (for Nix-level consumption).
    # Access: graph.system.build.toplevel.__node.{deps,value}
    graph = toNested (e: { inherit (e) deps value; });

    # JSON-serializable: deps only (no values, which may contain derivations).
    # Access: depsOnly.system.build.toplevel.__node.deps
    depsOnly = toNested (e: { inherit (e) deps; });

    # Nodes that have at least one dependency.
    nodesWithDeps = length (filter (e: e.deps != []) flatEntries);
    leafNodes = nodeCount - nodesWithDeps;

    # Full d2 graph (warning: 1600+ nodes, very large).
    d2 = mkD2 flatEntries;

    # Filtered: only interior nodes, edges pruned to stay within the set.
    d2Filtered =
      let
        interior = filter (e: e.deps != []) flatEntries;
        interiorKeys = listToAttrs (map (e: { name = pathKey e.path; value = true; }) interior);
        pruned = map (e: e // {
          deps = filter (d: interiorKeys ? ${pathKey d}) e.deps;
        }) interior;
      in mkD2 pruned;

    # --- Graphviz dot output -------------------------------------------------
    #
    # Usage:
    #   ./build/src/nix/nix eval --impure --raw -f ./nixpkgs/dep-graph.nix dot > graph.dot
    #   dot -Tsvg graph.dot -o graph.svg
    #
    # Same quoting strategy as d2: node IDs are the Nix-style quoted path
    # inside dot double-quotes, with inner quotes escaped for dot.

    dotEscape = s: lib.replaceStrings [ ''"'' "\\" ] [ ''\"'' "\\\\" ] s;

    dotNodeId = path: ''"'' + dotEscape (nixPath path) + ''"'';

    mkDot = entries:
      let
        nodeDecls = map (e:
          "  ${dotNodeId e.path};"
        ) entries;

        edges = concatMap (e:
          map (dep: "  ${dotNodeId e.path} -> ${dotNodeId dep};") e.deps
        ) entries;
      in
      concatStringsSep "\n" (
        [ "// NixOS dependency graph — ${toString (length entries)} nodes"
          "// Generated by dep-graph.nix"
          "digraph {"
          "  rankdir=LR;"
          ""
        ]
        ++ nodeDecls
        ++ [ "" ]
        ++ edges
        ++ [ "}" ]
      ) + "\n";

    # Full dot graph.
    dot = mkDot flatEntries;

    # Filtered: only interior nodes, edges pruned to stay within the set.
    dotFiltered =
      let
        interior = filter (e: e.deps != []) flatEntries;
        interiorKeys = listToAttrs (map (e: { name = pathKey e.path; value = true; }) interior);
        pruned = map (e: e // {
          deps = filter (d: interiorKeys ? ${pathKey d}) e.deps;
        }) interior;
      in mkDot pruned;
  }
