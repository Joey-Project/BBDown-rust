#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'usage: %s <binary-path> <package-name> <output-dir>\n' "$0" >&2
}

if [[ $# -ne 3 ]]; then
  usage
  exit 2
fi

binary_path=$1
package_name=$2
output_dir=$3

if [[ -z "$package_name" || "$package_name" == *[!A-Za-z0-9._-]* ]]; then
  printf 'package name must contain only letters, numbers, dot, underscore, or dash: %s\n' "$package_name" >&2
  exit 2
fi

if [[ ! -f "$binary_path" ]]; then
  printf 'binary path does not exist: %s\n' "$binary_path" >&2
  exit 2
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
mkdir -p "$output_dir"
output_dir=$(cd "$output_dir" && pwd)

staging_parent=$(mktemp -d)
trap 'rm -rf "$staging_parent"' EXIT
staging_dir="$staging_parent/$package_name"
archive_path="$output_dir/$package_name.tar.gz"
checksum_path="$archive_path.sha256"
archive_name=$(basename "$archive_path")

mkdir -p "$staging_dir/docs"
cp "$binary_path" "$staging_dir/bbdown"
chmod 0755 "$staging_dir/bbdown"
cp "$repo_root/README.md" "$staging_dir/README.md"
cp "$repo_root/docs/user-guide.md" "$staging_dir/docs/user-guide.md"
if [[ -f "$repo_root/LICENSE" ]]; then
  cp "$repo_root/LICENSE" "$staging_dir/LICENSE"
fi

tar -C "$staging_parent" -czf "$archive_path" "$package_name"
if command -v shasum >/dev/null 2>&1; then
  (cd "$output_dir" && shasum -a 256 "$archive_name" > "$checksum_path")
elif command -v sha256sum >/dev/null 2>&1; then
  (cd "$output_dir" && sha256sum "$archive_name" > "$checksum_path")
else
  printf 'neither shasum nor sha256sum is available\n' >&2
  exit 2
fi

printf '%s\n' "$archive_path"
