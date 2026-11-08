#!/bin/sh
set -eu

if [ "$#" -ne 2 ]; then
    printf '%s\n' 'Usage: check-install.sh BINARY ARCHIVE' >&2
    exit 2
fi
binary=$1
archive=$2
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
fixture=$(mktemp -d "${TMPDIR:-/tmp}/shimmer-install-check.XXXXXX")
trap 'rm -rf "$fixture"' EXIT HUP INT TERM
mkdir -p "$fixture/home/.config/shell" "$fixture/bundle"
printf '%s\n' 'effect=assemble' > "$fixture/home/.config/shell/shimmer.conf"
printf '%s\n' 'PS1="fixture> "' 'PROMPT_COMMAND=:' \
    '[[ -r ~/.config/shell/shimmer.bash ]] && source ~/.config/shell/shimmer.bash' \
    '[[ -r "$HOME/.config/shell/shimmer.bash" ]] && source "$HOME/.config/shell/shimmer.bash"' \
    'PROMPT_COMMAND="echo after-existing-source"' > "$fixture/home/.bashrc"
HOME=$fixture/home SHIMMER_BIN=$binary sh "$root/scripts/install.sh"
cp "$fixture/home/.bashrc" "$fixture/bashrc-first-install"
HOME=$fixture/home SHIMMER_BIN=$binary sh "$root/scripts/install.sh"
cmp "$fixture/bashrc-first-install" "$fixture/home/.bashrc"
[ "$(cat "$fixture/home/.config/shell/shimmer.conf")" = 'effect=assemble' ]
[ "$(grep -c '^\[\[ -r ~/.config/shell/shimmer.bash' "$fixture/home/.bashrc")" -eq 1 ]
hook_count=$(grep -c 'source .*shimmer.bash' "$fixture/home/.bashrc")
if [ "$hook_count" -ne 1 ]; then
    printf 'Expected one Shimmer source line, found %s\n' "$hook_count" >&2
    exit 1
fi
[ "$(tail -n 1 "$fixture/home/.bashrc")" = '[[ -r ~/.config/shell/shimmer.bash ]] && source ~/.config/shell/shimmer.bash' ]
grep -q '^PROMPT_COMMAND="echo after-existing-source"$' "$fixture/home/.bashrc"
"$fixture/home/.local/bin/shimmer" --version
HOME=$fixture/home bash --noprofile --norc -c '. "$HOME/.config/shell/shimmer.bash"'
tar -xzf "$archive" -C "$fixture/bundle"
set -- "$fixture"/bundle/shimmer-*
HOME=$fixture/fresh-home sh "$1/install.sh"
cmp "$1/shimmer" "$fixture/fresh-home/.local/bin/shimmer"
cmp "$1/scripts/shell-shimmer.bash" "$fixture/fresh-home/.config/shell/shimmer.bash"
cmp "$1/scripts/shell-shimmer.conf" "$fixture/fresh-home/.config/shell/shimmer.conf"
"$fixture/fresh-home/.local/bin/shimmer" --version
printf '%s\n' 'Source and release-bundle installation passed.'
