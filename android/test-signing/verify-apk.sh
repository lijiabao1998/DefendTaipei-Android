#!/usr/bin/env bash
# Verify the actual APK, not merely Gradle configuration. Public test signing only.
set -euo pipefail
if [[ $# -ne 1 || ! -f "$1" ]]; then
  printf 'Usage: %s APK\n' "$0" >&2
  exit 2
fi
apk=$1
fixture_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
expected=$(tr -d '[:space:]' < "$fixture_dir/certificate.sha256")
apksigner_bin=${APKSIGNER:-}
if [[ -z "$apksigner_bin" ]]; then
  apksigner_bin=$(command -v apksigner || true)
fi
if [[ -z "$apksigner_bin" ]]; then
  sdk_root=${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}
  if [[ -n "$sdk_root" ]]; then
    for candidate in "$sdk_root"/build-tools/*/apksigner; do
      if [[ -x "$candidate" ]]; then apksigner_bin=$candidate; fi
    done
  fi
fi
if [[ -z "$apksigner_bin" || ! -x "$apksigner_bin" ]]; then
  printf 'apksigner is required; set APKSIGNER or ANDROID_SDK_ROOT.\n' >&2
  exit 2
fi
if ! verification=$("$apksigner_bin" verify --verbose --print-certs-pem "$apk" 2>&1); then
  printf 'apksigner verification failed; public verification output:\n%s\n' "$verification" >&2
  exit 1
fi
actual=$(printf '%s\n' "$verification" |
  python3 "$fixture_dir/apksigner_output.py" --expected "$expected")
printf 'APK development signer verified: %s\n' "$actual"
