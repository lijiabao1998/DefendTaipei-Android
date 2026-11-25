#!/usr/bin/env bash
# Run the Android build with bounded backoff for remote repository failures.
# Compilation/package failures keep their original exit code and are not retried.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$script_dir/../android"
gradle_log="$(mktemp)"
trap 'rm -f "$gradle_log"' EXIT

retry_seconds="${GRADLE_RETRY_BASE_SECONDS:-30}"
if [[ ! "$retry_seconds" =~ ^[0-9]+$ ]]; then
  echo "GRADLE_RETRY_BASE_SECONDS must be a nonnegative integer" >&2
  exit 2
fi

for attempt in 1 2 3; do
  if ./gradlew --no-daemon --max-workers=2 "$@" 2>&1 | tee "$gradle_log"; then
    exit 0
  else
    pipeline_status=("${PIPESTATUS[@]}")
    build_status="${pipeline_status[0]}"
    if (( build_status == 0 )); then
      exit "${pipeline_status[1]}"
    fi
  fi

  if (( attempt == 3 )) || ! grep -Eq \
    'Received status code (429|502|503|504) from server|Server returned HTTP response code: (429|502|503|504)' \
    "$gradle_log"; then
    exit "$build_status"
  fi

  delay=$((retry_seconds * attempt))
  printf 'Dependency repository unavailable; retrying Gradle in %s seconds (%s/3).\n' \
    "$delay" "$((attempt + 1))" >&2
  sleep "$delay"
done
