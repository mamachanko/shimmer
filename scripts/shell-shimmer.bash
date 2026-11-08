# Source after PS1 and all PROMPT_COMMAND setup. One callback owns one effect.
if (( BASH_VERSINFO[0] < 5 || (BASH_VERSINFO[0] == 5 && BASH_VERSINFO[1] < 1) )); then
    return 0
fi

_shimmer_setup() {
    local effect=shimmer line declaration index
    if [[ -f ~/.config/shell/shimmer.conf && -r ~/.config/shell/shimmer.conf ]]; then
        while IFS= read -r line || [[ -n $line ]]; do
            if [[ $line =~ ^[[:space:]]*effect[[:space:]]*=[[:space:]]*(shimmer|assemble|off)[[:space:]]*$ ]]; then
                effect=${BASH_REMATCH[1]}
            fi
        done < ~/.config/shell/shimmer.conf
    fi
    case ${SHELL_SHIMMER_EFFECT:-} in
        shimmer|assemble|off) effect=$SHELL_SHIMMER_EFFECT ;;
    esac
    if [[ $- != *i* || ! -t 0 || ! -t 1 || ${TERM:-dumb} == dumb || ${SHELL_SHIMMER:-1} == 0 ]]; then
        effect=off
    fi
    if [[ $effect == off ]]; then
        if [[ ${_shimmer_pending_effect+x} ]]; then _shimmer_disarm; fi
        _shimmer_cleanup
        return 0
    fi
    if [[ ${_shimmer_pending_effect+x} ]]; then
        _shimmer_pending_effect=$effect
        return 0
    fi
    declaration=$(declare -p PROMPT_COMMAND 2>/dev/null) || declaration=
    if [[ $declaration =~ ^declare\ -[^[:space:]]*[rAn] ]]; then
        _shimmer_cleanup
        return 0
    fi
    declare -ga _shimmer_original_keys=() _shimmer_original_values=()
    declare -g _shimmer_original_kind=unset _shimmer_pending_effect=$effect
    declare -g _shimmer_original_exported=0
    if [[ $declaration =~ ^declare\ -[^[:space:]]*x ]]; then
        _shimmer_original_exported=1
    fi
    if [[ $declaration =~ ^declare\ -[^[:space:]]*a ]]; then
        _shimmer_original_kind=array
        _shimmer_original_keys=("${!PROMPT_COMMAND[@]}")
        _shimmer_original_values=("${PROMPT_COMMAND[@]}")
    elif [[ ${PROMPT_COMMAND+x} ]]; then
        _shimmer_original_kind=scalar
        _shimmer_original_values=("$PROMPT_COMMAND")
    fi
    unset PROMPT_COMMAND
    declare -ga PROMPT_COMMAND=("${_shimmer_original_values[@]}" _shimmer_once)
    if (( _shimmer_original_exported )); then export PROMPT_COMMAND; fi
    declare -g _shimmer_installed_hooks
    _shimmer_installed_hooks=$(declare -p PROMPT_COMMAND)
}

_shimmer_disarm() {
    local declaration index
    declaration=$(declare -p PROMPT_COMMAND 2>/dev/null) || declaration=
    if [[ $declaration =~ ^declare\ -[^[:space:]]*r ]]; then
        :
    elif [[ $declaration == "$_shimmer_installed_hooks" ]]; then
        unset PROMPT_COMMAND
        case $_shimmer_original_kind in
            scalar) declare -g PROMPT_COMMAND=${_shimmer_original_values[0]} ;;
            array)
                declare -ga PROMPT_COMMAND=()
                for index in "${!_shimmer_original_keys[@]}"; do
                    PROMPT_COMMAND[${_shimmer_original_keys[$index]}]=${_shimmer_original_values[$index]}
                done ;;
        esac
        if (( _shimmer_original_exported )); then export PROMPT_COMMAND; fi
    elif [[ $declaration =~ ^declare\ -[^[:space:]]*a ]]; then
        for index in "${!PROMPT_COMMAND[@]}"; do
            if [[ ${PROMPT_COMMAND[$index]} == _shimmer_once ]]; then
                unset 'PROMPT_COMMAND[index]'
            fi
        done
    elif [[ ${PROMPT_COMMAND-} == _shimmer_once ]]; then
        unset PROMPT_COMMAND
    fi
    unset _shimmer_pending_effect _shimmer_original_kind _shimmer_original_keys
    unset _shimmer_original_values _shimmer_original_exported _shimmer_installed_hooks
}

_shimmer_cleanup() {
    local declaration hook keep_once=
    declaration=$(declare -p PROMPT_COMMAND 2>/dev/null) || declaration=
    if [[ $declaration =~ ^declare\ -[^[:space:]]*r ]]; then
        for hook in "${PROMPT_COMMAND[@]}"; do
            if [[ $hook == _shimmer_once ]]; then keep_once=1; break; fi
        done
    fi
    if [[ $keep_once ]]; then
        # Bash cannot remove a callback from a hook another command made readonly.
        _shimmer_once() { return $?; }
    else
        unset -f _shimmer_once
    fi
    unset -f _shimmer_setup _shimmer_disarm _shimmer_cleanup _shimmer_render
}

_shimmer_once() {
    local status=$? effect=$_shimmer_pending_effect prompt=
    _shimmer_disarm
    if [[ $effect == assemble ]]; then
        if (exit "$status"); then
            prompt=${PS1@P}
        else
            prompt=${PS1@P}
        fi
    fi
    _shimmer_render "$effect" "$prompt"
    _shimmer_cleanup
    return "$status"
}

_shimmer_render() {
    local binary=${SHELL_SHIMMER_BIN:-$HOME/.local/bin/shimmer}
    if [[ ! -x $binary ]]; then
        printf 'shimmer: binary not found at %s; install shimmer or set SHELL_SHIMMER_BIN\n' "$binary" >&2
        return 0
    fi
    if "$binary" "$1" "$2"; then
        :
    fi
}

_shimmer_setup
unset -f _shimmer_setup
