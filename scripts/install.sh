#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ -f "$script_dir/scripts/shell-shimmer.bash" ]; then
    source_dir=$script_dir
else
    source_dir=$(CDPATH= cd -- "$script_dir/.." && pwd)
fi
binary=${SHIMMER_BIN:-$source_dir/shimmer}
if [ ! -x "$binary" ] && [ -z "${SHIMMER_BIN:-}" ]; then
    binary=$source_dir/target/release/shimmer
fi
if [ ! -x "$binary" ]; then
    printf '%s\n' "Shimmer binary not found: $binary" 'Build with make build, or extract a release bundle before installing.' >&2
    exit 1
fi
: "${HOME:?HOME must identify the installation destination}"
config_dir=$HOME/.config/shell
bin_dir=$HOME/.local/bin
mkdir -p "$config_dir" "$bin_dir"
install -m 755 "$binary" "$bin_dir/shimmer"
install -m 644 "$source_dir/scripts/shell-shimmer.bash" "$config_dir/shimmer.bash"
if [ ! -e "$config_dir/shimmer.conf" ]; then
    install -m 644 "$source_dir/scripts/shell-shimmer.conf" "$config_dir/shimmer.conf"
fi

bashrc=$HOME/.bashrc
touch "$bashrc"
temporary=$(mktemp "$HOME/.shimmer-bashrc.XXXXXX")
trap 'rm -f "$temporary"' EXIT HUP INT TERM
awk '
    $0 == "# Shimmer prompt animation" { next }
    $0 == "[[ -r ~/.config/shell/shimmer.bash ]] && source ~/.config/shell/shimmer.bash" { next }
    $0 == "[[ -r \"$HOME/.config/shell/shimmer.bash\" ]] && source \"$HOME/.config/shell/shimmer.bash\"" { next }
    { print }
' "$bashrc" > "$temporary"
printf '%s\n%s\n' '# Shimmer prompt animation' \
    '[[ -r ~/.config/shell/shimmer.bash ]] && source ~/.config/shell/shimmer.bash' >> "$temporary"
cat "$temporary" > "$bashrc"
printf '%s\n' "Installed $bin_dir/shimmer and $config_dir/shimmer.bash." \
    'Open a new interactive Bash 5.1+ shell to run the selected effect.'
