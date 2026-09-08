# Source this file from ~/.bashrc after adding the cm executable to PATH.
# Explicit cm -e calls run in this shell. Missing commands run synchronously
# in Bash's command-not-found environment and return their actual exit status.

_cm_source_file() {
    local _cm_source_path=$1 _cm_source_status
    local _CM_IN_PARENT_EXEC=1
    shift
    builtin source "$_cm_source_path"
    _cm_source_status=$?
    command rm -f -- "$_cm_source_path"
    return "$_cm_source_status"
}

cm() {
    local _cm_source_path _cm_request_status
    if ! type -P cm >/dev/null 2>&1; then
        printf 'cm: executable not found in PATH\n' >&2
        return 127
    fi
    _cm_source_path=$(command mktemp "${TMPDIR:-/tmp}/cm-source.XXXXXXXX") || return
    if SHELL="$BASH" command cm --shell-output "$_cm_source_path" "$@"; then
        if [[ -s $_cm_source_path ]]; then
            _cm_source_file "$_cm_source_path"
            return $?
        fi
        command rm -f -- "$_cm_source_path"
        return 0
    else
        _cm_request_status=$?
        command rm -f -- "$_cm_source_path"
        return "$_cm_request_status"
    fi
}

cm_command_not_found_handler() {
    local _cm_command_line _cm_source_path _cm_request_status
    if [[ ${_CM_IN_PARENT_EXEC:-0} == 1 ]] || ! type -P cm >/dev/null 2>&1; then
        printf 'command not found: %s\n' "$1" >&2
        return 127
    fi
    printf -v _cm_command_line '%q ' "$@"
    _cm_source_path=$(command mktemp "${TMPDIR:-/tmp}/cm-source.XXXXXXXX") || return
    # The heredoc supplies only the model request. Source outside its scope so
    # the generated command retains the original stdin (including pipe input).
    if SHELL="$BASH" command cm --shell-output "$_cm_source_path" -e <<CM_EOF
$_cm_command_line
CM_EOF
    then
        _cm_source_file "$_cm_source_path"
        return $?
    else
        _cm_request_status=$?
        command rm -f -- "$_cm_source_path"
        return "$_cm_request_status"
    fi
}

command_not_found_handle() {
    cm_command_not_found_handler "$@"
}

# Remove only our obsolete deferred hook when reloading an existing session.
_cm_remove_deferred_hook() {
    local _cm_hook_index
    if [[ $(declare -p PROMPT_COMMAND 2>/dev/null) == 'declare -a '* ]]; then
        for _cm_hook_index in "${!PROMPT_COMMAND[@]}"; do
            if [[ ${PROMPT_COMMAND[$_cm_hook_index]} == _cm_run_pending ]]; then
                unset "PROMPT_COMMAND[$_cm_hook_index]"
            fi
        done
    elif [[ ${PROMPT_COMMAND:-} == _cm_run_pending ]]; then
        unset PROMPT_COMMAND
    elif [[ ${PROMPT_COMMAND:-} == '_cm_run_pending; '* ]]; then
        PROMPT_COMMAND=${PROMPT_COMMAND#'_cm_run_pending; '}
    fi
    unset -f _cm_run_pending
    if [[ -n ${_CM_QUEUE_DIR:-} ]]; then
        command rmdir -- "$_CM_QUEUE_DIR" 2>/dev/null || :
    fi
    unset _CM_QUEUE_DIR _CM_PARENT_INTEGRATION_LOADED
}
_cm_remove_deferred_hook
unset -f _cm_remove_deferred_hook
