#!/bin/sh
set -eu

dist_dir=${1:-dist}
archive=$(find "$dist_dir" -maxdepth 1 -type f -name 'bellhound-*.tar.gz' -print -quit)
[ -n "$archive" ] || { echo "Bellhound archive not found in $dist_dir" >&2; exit 1; }
checksum=$archive.sha256
[ -f "$checksum" ] || { echo "checksum not found: $checksum" >&2; exit 1; }

if command -v shasum >/dev/null 2>&1; then
  (cd "$dist_dir" && shasum -a 256 -c "$(basename "$checksum")")
else
  (cd "$dist_dir" && sha256sum -c "$(basename "$checksum")")
fi

stage=$(mktemp -d "${TMPDIR:-/tmp}/bellhound-verify.XXXXXX")
trap 'rm -rf "$stage"' EXIT HUP INT TERM
tar -xzf "$archive" -C "$stage"
root=$(find "$stage" -mindepth 1 -maxdepth 1 -type d -print -quit)
[ -n "$root" ] || { echo 'archive root not found' >&2; exit 1; }

for path in bellhound INSTALL.md README.md LICENSE AUTHORS.md CHANGELOG.md CONTRIBUTING.md SECURITY.md bellhound.example.toml docs/platform-validation.md examples/crocodile-call.jsonl examples/mcp-status-only.toml scripts/verify-package-behavior.py; do
  [ -e "$root/$path" ] || { echo "archive member not found: $path" >&2; exit 1; }
done
[ -x "$root/bellhound" ] || { echo 'Bellhound binary is not executable' >&2; exit 1; }
"$root/bellhound" --version
"$root/bellhound" --config "$root/bellhound.example.toml" check
python3 "$root/scripts/verify-package-behavior.py" --binary "$root/bellhound" --root "$root"

if [ "$(uname -s)" = Darwin ]; then
  [ -x "$root/scripts/mcp-service-macos.sh" ] || { echo 'macOS service installer is not executable' >&2; exit 1; }
  PYTHONDONTWRITEBYTECODE=1 python3 "$root/scripts/mcp-service-supervisor.py" --help >/dev/null
fi

printf '%s\n' "verified $(basename "$archive")"
