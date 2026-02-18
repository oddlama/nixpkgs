{
  writeShellApplication,
  json2nix,
  nixfmt-rfc-style,
  difftastic,
  diffutils,
}:
writeShellApplication {
  name = "nixos-diff";
  runtimeInputs = [
    json2nix
    nixfmt-rfc-style
    difftastic
    diffutils
  ];
  text = ''
    explicit=false
    for arg in "$@"; do
      case $arg in
        --explicit) explicit=true; shift ;;
        --help|-h)
          echo "Usage: nixos-diff [--explicit] OLD NEW"
          echo ""
          echo "Diff two NixOS tracked toplevel configurations."
          echo ""
          echo "Options:"
          echo "  --explicit  Use tracking-explicit.json (only explicitly defined values)"
          echo "  --help      Show this help"
          exit 0
          ;;
        *) break ;;
      esac
    done

    old="''${1:?Usage: nixos-diff [--explicit] OLD NEW}"
    new="''${2:?Usage: nixos-diff [--explicit] OLD NEW}"

    file=tracking.json
    if $explicit; then
      file=tracking-explicit.json
    fi

    old_nix=$(mktemp --suffix=.nix)
    new_nix=$(mktemp --suffix=.nix)
    trap 'rm -f "$old_nix" "$new_nix"' EXIT

    json2nix "$old/$file" | nixfmt > "$old_nix"
    json2nix "$new/$file" | nixfmt > "$new_nix"

    rc=0
    if [ -t 1 ]; then
      difft "$old_nix" "$new_nix" || rc=$?
    else
      diff -u --label "a/$file" --label "b/$file" "$old_nix" "$new_nix" || rc=$?
    fi
    exit "$rc"
  '';
}
