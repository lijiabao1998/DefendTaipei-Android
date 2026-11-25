#!/usr/bin/env bash
# Verify retry boundaries without network requests or a real Android build.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
test_dir="$(mktemp -d)"
trap 'rm -rf "$test_dir"' EXIT
mkdir -p "$test_dir/scripts" "$test_dir/android"
cp "$script_dir/gradle-assemble.sh" "$test_dir/scripts/"
cat >"$test_dir/android/gradlew" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
count=0
[[ ! -f attempts ]] || count="$(cat attempts)"
count=$((count + 1))
printf '%s' "$count" >attempts
printf '%s\n' "$@" >args
case "$TEST_GRADLE_FAILURE" in
  transient)
    if (( count < 3 )); then
      echo 'Could not GET dependency. Received status code 429 from server: Too Many Requests'
      exit 7
    fi
    ;;
  persistent)
    echo 'Received status code 503 from server: Service Unavailable'
    exit 8
    ;;
  compiler)
    echo 'error: cannot find symbol'
    exit 42
    ;;
esac
SH
chmod +x "$test_dir/android/gradlew"

run_case() {
  local failure="$1" expected_status="$2" expected_attempts="$3" actual_status=0
  rm -f "$test_dir/android/attempts"
  TEST_GRADLE_FAILURE="$failure" GRADLE_RETRY_BASE_SECONDS=0 \
    bash "$test_dir/scripts/gradle-assemble.sh" assembleDebug -PABI_FILTERS=x86_64 \
    >"$test_dir/output" 2>&1 || actual_status=$?
  [[ "$actual_status" == "$expected_status" ]] || {
    cat "$test_dir/output" >&2
    echo "Unexpected exit code for $failure: $actual_status" >&2
    exit 1
  }
  [[ "$(cat "$test_dir/android/attempts")" == "$expected_attempts" ]]
  grep -Fxq -- '--max-workers=2' "$test_dir/android/args"
  grep -Fxq -- '-PABI_FILTERS=x86_64' "$test_dir/android/args"
}

run_case transient 0 3
run_case persistent 8 3
run_case compiler 42 1
echo 'Gradle backoff: recovery, retry limit, original exit codes and ABI arguments passed.'
