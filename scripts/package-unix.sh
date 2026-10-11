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
cp README.md AUTHORS.md LICENSE CHANGELOG.md "$stage/$archive_name/"
tar -C "$stage" -czf "$output_dir/$archive_name.tar.gz" "$archive_name"

if command -v shasum >/dev/null 2>&1; then
  (cd "$output_dir" && shasum -a 256 "$archive_name.tar.gz" > "$archive_name.tar.gz.sha256")
else
  (cd "$output_dir" && sha256sum "$archive_name.tar.gz" > "$archive_name.tar.gz.sha256")
fi

printf '%s\n' "$output_dir/$archive_name.tar.gz"
printf '%s\n' "$output_dir/$archive_name.tar.gz.sha256"
