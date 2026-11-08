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
printf '%s\n' 'effect=assemble' > "$fixture/home/.config/shell/glimmer.conf"
printf '%s\n' 'PS1="fixture> "' 'PROMPT_COMMAND=:' \
    '[[ -r ~/.config/shell/glimmer.bash ]] && source ~/.config/shell/glimmer.bash' \
    '[[ -r "$HOME/.config/shell/glimmer.bash" ]] && source "$HOME/.config/shell/glimmer.bash"' \
    'PROMPT_COMMAND="echo after-existing-source"' > "$fixture/home/.bashrc"
HOME=$fixture/home SHIMMER_BIN=$binary sh "$root/scripts/install.sh"
cp "$fixture/home/.bashrc" "$fixture/bashrc-first-install"
HOME=$fixture/home SHIMMER_BIN=$binary sh "$root/scripts/install.sh"
cmp "$fixture/bashrc-first-install" "$fixture/home/.bashrc"
[ "$(cat "$fixture/home/.config/shell/glimmer.conf")" = 'effect=assemble' ]
[ "$(grep -c '^\[\[ -r ~/.config/shell/glimmer.bash' "$fixture/home/.bashrc")" -eq 1 ]
hook_count=$(grep -c 'source .*glimmer.bash' "$fixture/home/.bashrc")
if [ "$hook_count" -ne 1 ]; then
    printf 'Expected one Shimmer source line, found %s\n' "$hook_count" >&2
    exit 1
fi
[ "$(tail -n 1 "$fixture/home/.bashrc")" = '[[ -r ~/.config/shell/glimmer.bash ]] && source ~/.config/shell/glimmer.bash' ]
grep -q '^PROMPT_COMMAND="echo after-existing-source"$' "$fixture/home/.bashrc"
"$fixture/home/.local/bin/shimmer" --version
HOME=$fixture/home bash --noprofile --norc -c '. "$HOME/.config/shell/glimmer.bash"'
tar -xzf "$archive" -C "$fixture/bundle"
set -- "$fixture"/bundle/shimmer-*
HOME=$fixture/fresh-home sh "$1/install.sh"
cmp "$1/shimmer" "$fixture/fresh-home/.local/bin/shimmer"
cmp "$1/scripts/shell-glimmer.bash" "$fixture/fresh-home/.config/shell/glimmer.bash"
cmp "$1/scripts/shell-glimmer.conf" "$fixture/fresh-home/.config/shell/glimmer.conf"
"$fixture/fresh-home/.local/bin/shimmer" --version
printf '%s\n' 'Source and release-bundle installation passed.'
