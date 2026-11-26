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
verification=$("$apksigner_bin" verify --verbose --print-certs "$apk")
python3 -c '
import re, sys
expected, output = sys.argv[1:]
if not re.fullmatch(r"[a-f0-9]{64}", expected):
    raise SystemExit("Invalid checked-in development certificate fingerprint")
actual = re.findall(r"^Signer #[0-9]+ certificate SHA-256 digest: ([A-Fa-f0-9]{64})\s*$", output, re.M)
if len(actual) != 1 or actual[0].lower() != expected:
    raise SystemExit("APK does not have the single expected public development signer: " + str(actual))
print("APK development signer verified: " + expected)
' "$expected" "$verification"
