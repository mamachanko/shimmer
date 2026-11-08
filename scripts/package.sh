#!/bin/sh
set -eu

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    printf '%s\n' 'Usage: scripts/package.sh VERSION TARGET [BINARY]' >&2
    exit 2
fi
version=$1
target=$2
for value in "$version" "$target"; do
    case $value in
        ''|*[!a-zA-Z0-9._-]*) printf '%s\n' "Invalid package identifier: $value" >&2; exit 2 ;;
    esac
done
source_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
binary=${3:-$source_dir/target/release/shimmer}
if [ ! -x "$binary" ]; then
    printf '%s\n' "Shimmer binary not found: $binary" >&2
    exit 1
fi
output_dir=$source_dir/dist
name=shimmer-$version-$target
archive=shimmer-$target.tar.gz
mkdir -p "$output_dir"
staging=$(mktemp -d "${TMPDIR:-/tmp}/shimmer-package.XXXXXX")
trap 'rm -rf "$staging"' EXIT HUP INT TERM
mkdir -p "$staging/$name/scripts" "$staging/$name/docs"
install -m 755 "$binary" "$staging/$name/shimmer"
install -m 755 "$source_dir/scripts/install.sh" "$staging/$name/install.sh"
install -m 644 "$source_dir/scripts/shell-glimmer.bash" "$source_dir/scripts/shell-glimmer.conf" "$staging/$name/scripts/"
install -m 644 "$source_dir/README.md" "$source_dir/LICENSE" "$staging/$name/"
install -m 644 "$source_dir/docs/shell-glimmer.md" "$staging/$name/docs/"
tar -czf "$output_dir/$archive" -C "$staging" "$name"
(
    cd "$output_dir"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$archive" > "$archive.sha256"
    else
        shasum -a 256 "$archive" > "$archive.sha256"
    fi
)
printf '%s\n' "$output_dir/$archive"
