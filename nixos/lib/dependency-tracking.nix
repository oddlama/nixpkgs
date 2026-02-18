# Post-processing for NixOS fixpoint dependency tracking.
#
# When evalModules runs with `trackDependencies = true`, it produces raw
# dependency edges via `_dependencyTracking.getDependencies`. This module
# transforms those raw edges into filtered graphs, DOT visualizations,
# and serialized config values.
#
# Usage (from eval-config.nix):
#   processDependencyTracking = import ./dependency-tracking.nix { inherit lib; };
#   dependencyTracking = processDependencyTracking configuration;
#
# The returned attribute set is fully lazy — accessing e.g. `filteredDotOutput`
# only forces the filtering and DOT generation, not configValues.
#
# IMPORTANT: The caller must force evaluation of their target (e.g.
# `system.build.toplevel`) BEFORE accessing any tracking attributes, since
# `getDependencies` snapshots whatever has been recorded at the time it's forced.

{ lib }:

configuration:

let
  # Source data from the evalModules result
  tracking = configuration._dependencyTracking;
  config = tracking.rawConfig;
  options = configuration.options;

  hasTryCatchAll = builtins ? tryCatchAll;

  # =========================================================================
  # 1. Raw dependency deduplication
  # =========================================================================

  rawDeps =
    let
      raw = tracking.getDependencies;
      edgeKey = dep: builtins.toJSON [ dep.accessor dep.accessed ];
      grouped = builtins.groupBy edgeKey raw;
    in
    map (group: builtins.head group) (builtins.attrValues grouped);

  # =========================================================================
  # 2. Path formatting (Nix-style dot notation)
  # =========================================================================

  # Format a path component for Nix-style display.
  # Components matching [a-zA-Z_][0-9a-zA-Z_-]* are bare; others are quoted.
  formatComponent = c:
    if builtins.match "[a-zA-Z_][0-9a-zA-Z_-]*" c != null
    then c
    else "\"" + builtins.replaceStrings ["\\" "\""] ["\\\\" "\\\""] c + "\"";

  # Format a full path as Nix-style dot notation (e.g. services.nginx.enable
  # or fileSystems."/".device or systemd.services."getty@".enable)
  formatPath = path: lib.concatStringsSep "." (map formatComponent path);

  # Escape a string for embedding inside DOT "..." strings.
  dotEscape = s: builtins.replaceStrings ["\\" "\""] ["\\\\" "\\\""] s;

  # =========================================================================
  # 3. DOT visualization helpers
  # =========================================================================

  # Section colors for DOT graph visualization.
  sectionStyle = toplevel:
    let styles = {
      services       = { fill = "#4e79a7"; font = "white"; };   # steel blue
      systemd        = { fill = "#59a14f"; font = "white"; };   # green
      boot           = { fill = "#e15759"; font = "white"; };   # red
      networking     = { fill = "#f28e2b"; font = "white"; };   # orange
      users          = { fill = "#b07aa1"; font = "white"; };   # purple
      security       = { fill = "#ff9da7"; font = "black"; };   # pink
      environment    = { fill = "#9c755f"; font = "white"; };   # brown
      hardware       = { fill = "#bab0ac"; font = "black"; };   # gray
      system         = { fill = "#76b7b2"; font = "black"; };   # teal
      nix            = { fill = "#edc948"; font = "black"; };   # yellow
      nixpkgs        = { fill = "#edc948"; font = "black"; };   # yellow
      programs       = { fill = "#af7aa1"; font = "white"; };   # lavender
      _module        = { fill = "#aec7e8"; font = "black"; };   # light blue (pkgs)
      fileSystems    = { fill = "#d4a373"; font = "black"; };   # tan
      virtualisation = { fill = "#8cd17d"; font = "black"; };   # lime
      documentation  = { fill = "#b6992d"; font = "white"; };   # dark gold
      assertions     = { fill = "#888888"; font = "white"; };   # dark gray
      warnings       = { fill = "#888888"; font = "white"; };   # dark gray
    };
    in styles.${toplevel} or { fill = "#d3d3d3"; font = "black"; };

  # Collect unique nodes from an edge list and emit DOT node declarations with colors.
  dotNodeDecls = edges:
    let
      allPaths = lib.concatMap (e: [ e.accessor e.accessed ]) edges;
      uniqueNodes = builtins.attrValues (builtins.listToAttrs (
        map (p: { name = dotEscape (formatPath p); value = p; }) allPaths
      ));
    in
    lib.concatMapStringsSep "\n" (path:
      let
        label = dotEscape (formatPath path);
        top = builtins.head path;
        style = sectionStyle top;
      in
      "  \"${label}\" [style=filled, fillcolor=\"${style.fill}\", fontcolor=\"${style.font}\"];"
    ) uniqueNodes;

  # Generate a DOT digraph from a list of edges.
  makeDotOutput = edges: ''
    digraph dependencies {
      rankdir=LR;
      node [shape=box, fontsize=10];
      edge [fontsize=8, color="#666666"];

    ${dotNodeDecls edges}

    ${lib.concatMapStringsSep "\n" (dep:
      let
        accessor = dotEscape (formatPath dep.accessor);
        accessed = dotEscape (formatPath dep.accessed);
      in
      "  \"${accessor}\" -> \"${accessed}\";"
    ) edges}
    }
  '';

  # =========================================================================
  # 4. Option path collection
  # =========================================================================

  # Recursive walk: stop at _type = "option" nodes, use tryEval for robustness.
  collectOptionPaths =
    let
      walk = prefix: node:
        let
          res = builtins.tryEval (
            if builtins.isAttrs node && (node._type or "") == "option" then
              [ prefix ]
            else if builtins.isAttrs node then
              lib.concatLists (lib.mapAttrsToList (name: child: walk (prefix ++ [ name ]) child) node)
            else
              [ ]
          );
        in
        if res.success then res.value else [ ];
    in
    walk [ ] options;

  # Tab-separated path key for O(1) lookups (attr names never contain tabs).
  pathKey = builtins.concatStringsSep "\t";

  optionPathSet = builtins.listToAttrs (
    map (p: { name = pathKey p; value = true; }) collectOptionPaths
  );

  # =========================================================================
  # 5. Node filtering
  # =========================================================================

  # Internal option-record attributes — always noise.
  optionInternalAttrs = lib.genAttrs [
    "_type" "type" "value" "isDefined" "definitions" "definitionsWithLocations"
    "files" "highestPrio" "loc" "description" "default" "defaultText" "example"
    "readOnly" "internal" "visible" "apply" "declarations" "options"
    "check" "nestedTypes" "deprecationMessage" "relatedPackages"
    "getSubOptions" "getSubModules" "substSubModules" "functor"
  ] (_: true);

  # _module.args.pkgs.* filtering:
  #   pkgs                        -> keep
  #   pkgs.lib.* / pkgs.config.*  -> drop
  #   pkgs.<pkg>                  -> keep
  #   pkgs.<pkg>.out / .outPath   -> keep
  #   pkgs.<anything deeper>      -> drop
  pkgsBlacklist = lib.genAttrs [ "lib" "config" ] (_: true);
  pkgsKeptOutputs = lib.genAttrs [ "out" "outPath" ] (_: true);

  isPkgsKept = path:
    let
      len = builtins.length path;
      depth = len - 3; # components after ["_module" "args" "pkgs"]
    in
    if depth <= 0 then true
    else if depth == 1 then !(pkgsBlacklist ? ${builtins.elemAt path 3})
    else if depth == 2 then
      let pkg = builtins.elemAt path 3;
          sub = builtins.elemAt path 4;
      in !(pkgsBlacklist ? ${pkg}) && pkgsKeptOutputs ? ${sub}
    else false;

  isKeptNode = path:
    let
      len = builtins.length path;
      isModuleArgs = len >= 2
        && builtins.elemAt path 0 == "_module"
        && builtins.elemAt path 1 == "args";
      isPkgsPath = isModuleArgs && len >= 3
        && builtins.elemAt path 2 == "pkgs";
    in
    if isPkgsPath then isPkgsKept path
    else if isModuleArgs then false
    else
      let
        longestMatch = builtins.foldl'
          (best: i: if optionPathSet ? ${pathKey (lib.take i path)} then i else best)
          0
          (lib.range 1 len);
      in
      longestMatch > 0
      && (longestMatch == len
          || !(optionInternalAttrs ? ${builtins.elemAt path longestMatch}));

  # =========================================================================
  # 6. Node partitioning
  # =========================================================================

  allNodes =
    let
      allPaths = map (dep: dep.accessor) rawDeps ++ map (dep: dep.accessed) rawDeps;
      grouped = builtins.groupBy pathKey allPaths;
    in
    map (group: builtins.head group) (builtins.attrValues grouped);

  keptNodes = builtins.filter isKeptNode allNodes;

  keptNodeSet = builtins.listToAttrs (
    map (p: { name = pathKey p; value = true; }) keptNodes
  );

  # =========================================================================
  # 7. Transitive closure through pruned intermediates
  # =========================================================================

  # Adjacency map: accessor key -> list of accessed keys
  adjacencyMap =
    let
      edgesWithKeys = map (dep: {
        srcKey = pathKey dep.accessor;
        dstKey = pathKey dep.accessed;
      }) rawDeps;
      grouped = builtins.groupBy (e: e.srcKey) edgesWithKeys;
    in
    builtins.mapAttrs (_: edges: map (e: e.dstKey) edges) grouped;

  # For each kept source, BFS through pruned intermediates to find reachable kept targets.
  reachableKeptTargets = sourceKey:
    let
      closure = builtins.genericClosure {
        startSet = map (k: { key = k; }) (adjacencyMap.${sourceKey} or [ ]);
        operator = item:
          if keptNodeSet ? ${item.key} then
            [ ] # stop at kept nodes
          else
            map (k: { key = k; }) (adjacencyMap.${item.key} or [ ]);
      };
    in
    builtins.filter (item: keptNodeSet ? ${item.key} && item.key != sourceKey) closure;

  filteredDeps =
    let
      keptSourceKeys = builtins.filter (k: adjacencyMap ? ${k}) (map pathKey keptNodes);
      rawEdges = lib.concatMap (srcKey:
        map (item: {
          accessor = lib.splitString "\t" srcKey;
          accessed = lib.splitString "\t" item.key;
        }) (reachableKeptTargets srcKey)
      ) keptSourceKeys;
      grouped = builtins.groupBy (e:
        builtins.toJSON [ e.accessor e.accessed ]
      ) rawEdges;
    in
    map (group: builtins.head group) (builtins.attrValues grouped);

  # =========================================================================
  # 8. Graph-leaf detection
  # =========================================================================

  # A node is a "leaf" if no other kept node is a descendant of it.
  parentKeySet =
    let
      allPrefixes = lib.concatMap (p:
        let len = builtins.length p;
        in map (i: pathKey (lib.take i p)) (lib.range 1 (len - 1))
      ) keptNodes;
    in
    builtins.listToAttrs (map (k: { name = k; value = true; }) allPrefixes);

  # Leaf nodes: kept nodes with no descendants in the graph.
  # Excludes _module.args.* and renamed/obsolete options (visible = false).
  leafNodes = builtins.filter (p:
    let
      key = pathKey p;
      isModuleArgs = builtins.length p >= 2
        && builtins.elemAt p 0 == "_module"
        && builtins.elemAt p 1 == "args";
      # Check if this is a renamed/obsolete option (visible = false).
      # NOTE: use tryEval (shallow), NOT tryCatchAll — tryCatchAll deeply
      # evaluates the entire option record including .value, which triggers
      # renamed option trace messages via the apply callback.
      isRenamedOption =
        if optionPathSet ? ${key} then
          let res = builtins.tryEval (lib.attrByPath p null options);
          in res.success
             && res.value != null
             && (res.value.visible or true) == false
        else
          false;
    in
    !(parentKeySet ? ${key})
    && !isModuleArgs
    && !isRenamedOption
  ) keptNodes;

  # =========================================================================
  # 9. Config value serialization
  # =========================================================================

  sanitizeValue = depth: value:
    if depth <= 0 then
      "<depth-limit>"
    else
      let t = builtins.typeOf value;
      in
      if t == "string" then
        builtins.unsafeDiscardStringContext value
      else if t == "path" then
        "<path:${toString value}>"
      else if t == "lambda" then
        "<function>"
      else if t == "list" then
        map (sanitizeValue (depth - 1)) value
      else if t == "set" then
        if lib.isDerivation value then
          "<derivation:${value.name or "unknown"}>"
        else if builtins.length (builtins.attrNames value) > 50 then
          "<attrset:${toString (builtins.length (builtins.attrNames value))} attrs>"
        else
          lib.mapAttrs (_: sanitizeValue (depth - 1)) value
      else
        value; # int, float, bool, null pass through

  _missing = { _isMissing = true; };

  # Evaluate a single leaf path to a sanitized value, or _missing on failure.
  # Uses tryCatchAll (catches ALL errors) when available, falls back to tryEval.
  evalLeaf = path:
    let
      doEval =
        let
          val = lib.attrByPath path _missing config;
        in
        if val == _missing then _missing else sanitizeValue 5 val;
      evalResult =
        if hasTryCatchAll
        then builtins.tryCatchAll doEval
        else builtins.tryEval doEval;
    in
    if evalResult.success && evalResult.value != _missing
    then evalResult.value
    else _missing;

  # Build a nested attrset from { path; value; } entries by grouping
  # on first path component and recursing (avoids stack overflow from
  # foldl' + recursiveUpdate with thousands of paths).
  buildTree = entries:
    let
      leaves = builtins.filter (e: builtins.length e.path == 1) entries;
      branches = builtins.filter (e: builtins.length e.path > 1) entries;
      grouped = builtins.groupBy (e: builtins.head e.path) branches;
      subTrees = builtins.mapAttrs (_: subEntries:
        buildTree (map (e: {
          path = builtins.tail e.path;
          inherit (e) value;
        }) subEntries)
      ) grouped;
      leafAttrs = builtins.listToAttrs (map (e: {
        name = builtins.head e.path;
        inherit (e) value;
      }) leaves);
    in
    leafAttrs // subTrees;

  # Build configValues from a list of leaf node paths.
  buildConfigValues = leaves:
    let
      entries = builtins.concatMap (path:
        let val = evalLeaf path;
        in if val != _missing then [{ inherit path; value = val; }] else []
      ) leaves;
    in
    buildTree entries;

  configValues = buildConfigValues leafNodes;

  # =========================================================================
  # 10. Explicit definition filtering
  # =========================================================================

  # Check if a leaf path represents an explicitly defined value.
  # - Direct option paths: checks option.isDefined
  # - Sub-paths of container options (attrsOf, etc.): checks if any
  #   definition of the parent option includes the sub-path.  This filters
  #   out sub-option defaults that were never set by any module.
  #
  # The entire check is wrapped in tryCatchAll/tryEval because accessing
  # option.definitions can trigger evaluation cascades (e.g., discharging
  # mkIf conditions) that may throw on unrelated options.
  isExplicitlyDefined = path:
    let
      len = builtins.length path;
      longestMatch = builtins.foldl'
        (best: i: if optionPathSet ? ${pathKey (lib.take i path)} then i else best)
        0
        (lib.range 1 len);
      doCheck =
        if longestMatch == 0 then
          true  # Not under any known option (e.g., pkgs refs) — keep
        else if longestMatch == len then
          # Direct option path — check if it has any definitions
          let opt = lib.attrByPath path null options;
          in opt != null && (opt.isDefined or false)
        else
          # Sub-path of a container option (e.g., users.users.vaultwarden.isSystemUser)
          # Check if any definition of the parent option includes this sub-path.
          # Definitions contain the raw values from modules (after mkIf/mkMerge
          # processing), so sub-option defaults from the submodule declaration
          # do NOT appear here — only attributes explicitly set by modules.
          let
            optionPath = lib.take longestMatch path;
            subPath = lib.drop longestMatch path;
            opt = lib.attrByPath optionPath null options;
          in
          opt != null
          && builtins.any (defValue:
            lib.hasAttrByPath subPath defValue
          ) (opt.definitions or []);
      res = if hasTryCatchAll
        then builtins.tryCatchAll doCheck
        else builtins.tryEval doCheck;
    in
    res.success && res.value;

  explicitLeafNodes = builtins.filter isExplicitlyDefined leafNodes;

  explicitConfigValues = buildConfigValues explicitLeafNodes;

# =========================================================================
# Public interface
# =========================================================================
in
{
  # Raw deduplicated dependency edges: [ { accessor : [String]; accessed : [String]; } ]
  inherit rawDeps;

  # Filtered edges after node filtering + transitive closure
  inherit filteredDeps;

  # Kept and leaf node path lists
  inherit keptNodes leafNodes explicitLeafNodes;

  # Nested attrset of serialized config values for leaf nodes (JSON-safe)
  inherit configValues;

  # Like configValues, but only includes options that were explicitly defined
  # by a module (filters out sub-option defaults in attrsOf/submodule types)
  inherit explicitConfigValues;

  # DOT graph of all raw dependencies
  rawDotOutput = makeDotOutput rawDeps;

  # DOT graph of filtered dependencies (with section colors and Nix-style paths)
  inherit (let dot = makeDotOutput filteredDeps; in { filteredDotOutput = dot; }) filteredDotOutput;

  # Path formatting utility
  inherit formatPath;

  # Summary counts
  counts = {
    rawDeps = builtins.length rawDeps;
    filteredDeps = builtins.length filteredDeps;
    options = builtins.length collectOptionPaths;
    keptNodes = builtins.length keptNodes;
    leafNodes = builtins.length leafNodes;
    explicitLeafNodes = builtins.length explicitLeafNodes;
  };
}
