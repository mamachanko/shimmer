# Source after PS1 and all PROMPT_COMMAND setup. One callback owns one effect.
if (( BASH_VERSINFO[0] < 5 || (BASH_VERSINFO[0] == 5 && BASH_VERSINFO[1] < 1) )); then
    return 0
fi

_glimmer_setup() {
    local effect=shimmer line declaration index
    if [[ -f ~/.config/shell/glimmer.conf && -r ~/.config/shell/glimmer.conf ]]; then
        while IFS= read -r line || [[ -n $line ]]; do
            if [[ $line =~ ^[[:space:]]*effect[[:space:]]*=[[:space:]]*(shimmer|assemble|off)[[:space:]]*$ ]]; then
                effect=${BASH_REMATCH[1]}
            fi
        done < ~/.config/shell/glimmer.conf
    fi
    case ${SHELL_GLIMMER_EFFECT:-} in
        shimmer|assemble|off) effect=$SHELL_GLIMMER_EFFECT ;;
    esac
    if [[ $- != *i* || ! -t 0 || ! -t 1 || ${TERM:-dumb} == dumb || ${SHELL_GLIMMER:-1} == 0 ]]; then
        effect=off
    fi
    if [[ $effect == off ]]; then
        if [[ ${_glimmer_pending_effect+x} ]]; then _glimmer_disarm; fi
        _glimmer_cleanup
        return 0
    fi
    if [[ ${_glimmer_pending_effect+x} ]]; then
        _glimmer_pending_effect=$effect
        return 0
    fi
    declaration=$(declare -p PROMPT_COMMAND 2>/dev/null) || declaration=
    if [[ $declaration =~ ^declare\ -[^[:space:]]*[rAn] ]]; then
        _glimmer_cleanup
        return 0
    fi
    declare -ga _glimmer_original_keys=() _glimmer_original_values=()
    declare -g _glimmer_original_kind=unset _glimmer_pending_effect=$effect
    declare -g _glimmer_original_exported=0
    if [[ $declaration =~ ^declare\ -[^[:space:]]*x ]]; then
        _glimmer_original_exported=1
    fi
    if [[ $declaration =~ ^declare\ -[^[:space:]]*a ]]; then
        _glimmer_original_kind=array
        _glimmer_original_keys=("${!PROMPT_COMMAND[@]}")
        _glimmer_original_values=("${PROMPT_COMMAND[@]}")
    elif [[ ${PROMPT_COMMAND+x} ]]; then
        _glimmer_original_kind=scalar
        _glimmer_original_values=("$PROMPT_COMMAND")
    fi
    unset PROMPT_COMMAND
    declare -ga PROMPT_COMMAND=("${_glimmer_original_values[@]}" _glimmer_once)
    if (( _glimmer_original_exported )); then export PROMPT_COMMAND; fi
    declare -g _glimmer_installed_hooks
    _glimmer_installed_hooks=$(declare -p PROMPT_COMMAND)
}

_glimmer_disarm() {
    local declaration index
    declaration=$(declare -p PROMPT_COMMAND 2>/dev/null) || declaration=
    if [[ $declaration =~ ^declare\ -[^[:space:]]*r ]]; then
        :
    elif [[ $declaration == "$_glimmer_installed_hooks" ]]; then
        unset PROMPT_COMMAND
        case $_glimmer_original_kind in
            scalar) declare -g PROMPT_COMMAND=${_glimmer_original_values[0]} ;;
            array)
                declare -ga PROMPT_COMMAND=()
                for index in "${!_glimmer_original_keys[@]}"; do
                    PROMPT_COMMAND[${_glimmer_original_keys[$index]}]=${_glimmer_original_values[$index]}
                done ;;
        esac
        if (( _glimmer_original_exported )); then export PROMPT_COMMAND; fi
    elif [[ $declaration =~ ^declare\ -[^[:space:]]*a ]]; then
        for index in "${!PROMPT_COMMAND[@]}"; do
            if [[ ${PROMPT_COMMAND[$index]} == _glimmer_once ]]; then
                unset 'PROMPT_COMMAND[index]'
            fi
        done
    elif [[ ${PROMPT_COMMAND-} == _glimmer_once ]]; then
        unset PROMPT_COMMAND
    fi
    unset _glimmer_pending_effect _glimmer_original_kind _glimmer_original_keys
    unset _glimmer_original_values _glimmer_original_exported _glimmer_installed_hooks
}

_glimmer_cleanup() {
    local declaration hook keep_once=
    declaration=$(declare -p PROMPT_COMMAND 2>/dev/null) || declaration=
    if [[ $declaration =~ ^declare\ -[^[:space:]]*r ]]; then
        for hook in "${PROMPT_COMMAND[@]}"; do
            if [[ $hook == _glimmer_once ]]; then keep_once=1; break; fi
        done
    fi
    if [[ $keep_once ]]; then
        # Bash cannot remove a callback from a hook another command made readonly.
        _glimmer_once() { return $?; }
    else
        unset -f _glimmer_once
    fi
    unset -f _glimmer_setup _glimmer_disarm _glimmer_cleanup _glimmer_render
}

_glimmer_once() {
    local status=$? effect=$_glimmer_pending_effect prompt=
    _glimmer_disarm
    if [[ $effect == assemble ]]; then
        if (exit "$status"); then
            prompt=${PS1@P}
        else
            prompt=${PS1@P}
        fi
    fi
    _glimmer_render "$effect" "$prompt"
    _glimmer_cleanup
    return "$status"
}

_glimmer_render() {
    local binary=${SHELL_GLIMMER_BIN:-$HOME/.local/bin/shimmer}
    if [[ ! -x $binary ]]; then
        printf 'shimmer: binary not found at %s; install shimmer or set SHELL_GLIMMER_BIN\n' "$binary" >&2
        return 0
    fi
    if "$binary" "$1" "$2"; then
        :
    fi
}

_glimmer_setup
unset -f _glimmer_setup
