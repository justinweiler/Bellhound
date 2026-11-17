#!/bin/sh
set -eu

target=${1:-$(rustc -vV | awk '/^host:/ {print $2}')}
version=$(awk -F '"' '/^version = / {print $2; exit}' Cargo.toml)
archive_name="bellhound-${version}-${target}"
output_dir=${BELLHOUND_DIST_DIR:-dist}
stage=$(mktemp -d "${TMPDIR:-/tmp}/bellhound-package.XXXXXX")
trap 'rm -rf "$stage"' EXIT HUP INT TERM

cargo build --release --locked --target "$target"
mkdir -p "$output_dir" "$stage/$archive_name"
cp "target/$target/release/bellhound" "$stage/$archive_name/"
cp README.md INSTALL.md AUTHORS.md LICENSE CHANGELOG.md CONTRIBUTING.md SECURITY.md server.json "$stage/$archive_name/"
cp bellhound.example.toml "$stage/$archive_name/"
cp -R assets docs examples scripts skills "$stage/$archive_name/"
tar -C "$stage" -czf "$output_dir/$archive_name.tar.gz" "$archive_name"

if command -v shasum >/dev/null 2>&1; then
  (cd "$output_dir" && shasum -a 256 "$archive_name.tar.gz" > "$archive_name.tar.gz.sha256")
else
  (cd "$output_dir" && sha256sum "$archive_name.tar.gz" > "$archive_name.tar.gz.sha256")
fi

printf '%s\n' "$output_dir/$archive_name.tar.gz"
printf '%s\n' "$output_dir/$archive_name.tar.gz.sha256"
