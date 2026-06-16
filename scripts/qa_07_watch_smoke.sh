#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/cobble-qa-07-watch.XXXXXX")"
cleanup() {
  if [[ "${watch_pid:-}" =~ ^[0-9]+$ ]] && kill -0 "$watch_pid" 2>/dev/null; then
    kill "$watch_pid" 2>/dev/null || true
    wait "$watch_pid" 2>/dev/null || true
  fi
  if [[ "${COBBLE_QA_KEEP:-}" == "1" ]]; then
    echo "Keeping QA work directory: $work_dir"
  else
    rm -rf "$work_dir"
  fi
}
trap cleanup EXIT

run() {
  echo "+ $*"
  "$@"
}

wait_for_log() {
  local pattern="$1"
  local timeout_secs="$2"
  local elapsed=0
  while (( elapsed < timeout_secs )); do
    if grep -q "$pattern" "$log_file"; then
      return 0
    fi
    if ! kill -0 "$watch_pid" 2>/dev/null; then
      echo "watch exited while waiting for log pattern: $pattern" >&2
      cat "$log_file" >&2
      exit 1
    fi
    sleep 1
    elapsed=$((elapsed + 1))
  done

  echo "timed out waiting for log pattern: $pattern" >&2
  cat "$log_file" >&2
  exit 1
}

project_dir="$work_dir/watch_pack"
world_dir="$work_dir/world"
datapacks_dir="$world_dir/datapacks"
pack_dir="$datapacks_dir/watch_pack"
log_file="$work_dir/watch.log"
original_source="$work_dir/original-main.cbl"

run cargo run --locked --quiet -- init --name "$project_dir" --template validation
run cargo run --locked --quiet -- link "$project_dir" --datapacks "$datapacks_dir"
cp "$project_dir/src/main.cbl" "$original_source"

echo "+ cargo run --locked --quiet -- watch $project_dir/src --link --validate"
cargo run --locked --quiet -- watch "$project_dir/src" --link --validate >"$log_file" 2>&1 &
watch_pid=$!

wait_for_log "Initial build succeeded" 10

tracked_function="$pack_dir/data/watch_pack/function/init.mcfunction"
if [[ ! -f "$tracked_function" ]]; then
  echo "initial watch build did not write expected function: $tracked_function" >&2
  cat "$log_file" >&2
  exit 1
fi
previous_function="$work_dir/previous-init.mcfunction"
cp "$tracked_function" "$previous_function"

printf 'def init():\n    /titel @a actionbar bad\n' > "$project_dir/src/main.cbl"
wait_for_log "Build failed" 10
cmp "$previous_function" "$tracked_function"

cat "$original_source" > "$project_dir/src/main.cbl"
printf '\n# qa watcher recovery edit\n' >> "$project_dir/src/main.cbl"
wait_for_log "Build succeeded" 10

generated_noise="$pack_dir/data/watch_pack/function/generated_noise.cbl"
mkdir -p "$(dirname "$generated_noise")"
printf 'def ignored():\n    /say ignored\n' > "$generated_noise"
sleep 1

kill "$watch_pid" 2>/dev/null || true
wait "$watch_pid" 2>/dev/null || true
unset watch_pid

cat "$log_file"
grep -q "Initial build succeeded" "$log_file"
grep -q "Build failed" "$log_file"
grep -q "Build succeeded" "$log_file"
if grep -q "generated_noise.cbl" "$log_file"; then
  echo "watch reacted to generated output under the linked pack" >&2
  exit 1
fi

run cargo run --locked --quiet -- clean "$project_dir" --linked --dry-run
run cargo run --locked --quiet -- clean "$project_dir" --linked --yes

echo
echo "Watch smoke QA passed"
