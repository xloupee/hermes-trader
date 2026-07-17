#!/usr/bin/env bash

readonly WORKTREE_ROOT="$(git -C "$BASE_DIR" rev-parse --show-toplevel)"
if [[ "$WORKTREE_ROOT" != /srv/codex-workspaces/* ]]; then
  echo "Refusing NOXA runtime outside /srv/codex-workspaces: $WORKTREE_ROOT" >&2
  exit 1
fi
readonly RUNTIME_ROOT="$(realpath -m -- "$WORKTREE_ROOT/.runtime")"
if [[ "$RUNTIME_ROOT" != "$WORKTREE_ROOT/.runtime" ]]; then
  echo "Refusing runtime root that resolves outside the active worktree: $RUNTIME_ROOT" >&2
  exit 1
fi

canonical_runtime_path() {
  local requested="$1"
  local label="$2"
  local resolved
  resolved="$(realpath -m -- "$requested")"
  case "$resolved" in
    "$RUNTIME_ROOT"/*) printf '%s\n' "$resolved" ;;
    *)
      echo "Refusing $label outside $RUNTIME_ROOT: $resolved" >&2
      return 1
      ;;
  esac
}

canonical_child_path() {
  local requested="$1"
  local parent="$2"
  local label="$3"
  local resolved
  resolved="$(realpath -m -- "$requested")"
  case "$resolved" in
    "$parent"/*) printf '%s\n' "$resolved" ;;
    *)
      echo "Refusing $label outside $parent: $resolved" >&2
      return 1
      ;;
  esac
}

read_process_identity() {
  local pid="$1"
  [[ "$pid" =~ ^[1-9][0-9]*$ && "$pid" != 1 ]] || return 1
  local stat rest
  [[ -r "/proc/$pid/stat" ]] || return 1
  stat="$(<"/proc/$pid/stat")" || return 1
  rest="${stat##*) }"
  local state ppid pgid session tty_nr tpgid flags minflt cminflt
  local majflt cmajflt utime stime cutime cstime priority nice threads
  local itrealvalue starttime ignored
  read -r state ppid pgid session tty_nr tpgid flags minflt cminflt \
    majflt cmajflt utime stime cutime cstime priority nice threads \
    itrealvalue starttime ignored <<<"$rest"
  [[ "$pgid" =~ ^[1-9][0-9]*$ && "$starttime" =~ ^[0-9]+$ ]] || return 1
  printf '%s %s\n' "$pgid" "$starttime"
}

read_pid_record() {
  local pid_file="$1"
  local pid pgid starttime extra
  [[ -s "$pid_file" ]] || return 1
  read -r pid pgid starttime extra <"$pid_file" || return 1
  [[ -z "${extra:-}" ]] || return 1
  [[ "$pid" =~ ^[1-9][0-9]*$ && "$pid" != 1 ]] || return 1
  [[ "$pgid" == "$pid" && "$starttime" =~ ^[0-9]+$ ]] || return 1
  printf '%s %s %s\n' "$pid" "$pgid" "$starttime"
}

pid_record_is_live() {
  local pid="$1"
  local expected_pgid="$2"
  local expected_starttime="$3"
  local actual_pgid actual_starttime
  read -r actual_pgid actual_starttime < <(read_process_identity "$pid") || return 1
  [[ "$actual_pgid" == "$expected_pgid" && "$actual_starttime" == "$expected_starttime" ]]
}

write_current_pid_record() {
  local pid_file="$1"
  local pid="$$"
  local pgid starttime
  read -r pgid starttime < <(read_process_identity "$pid") || {
    echo "Could not read NOXA supervisor process identity" >&2
    return 1
  }
  if [[ "$pgid" != "$pid" ]]; then
    echo "NOXA supervisor PID $pid is not its process-group leader (PGID $pgid)" >&2
    return 1
  fi
  local temporary="$pid_file.tmp.$pid"
  printf '%s %s %s\n' "$pid" "$pgid" "$starttime" >"$temporary"
  chmod 600 "$temporary"
  mv -f -- "$temporary" "$pid_file"
}

remove_owned_pid_record() {
  local pid_file="$1"
  local pid pgid starttime
  [[ -s "$pid_file" ]] || return 0
  read -r pid pgid starttime < <(read_pid_record "$pid_file") || return 0
  if [[ "$pid" == "$$" ]]; then
    rm -f -- "$pid_file"
  fi
}
