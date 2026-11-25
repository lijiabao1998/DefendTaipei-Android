#!/usr/bin/env bash
# Install the debug APK on a booted emulator, run ~45 s, assert DTPERF + no crash.
# Expects: adb on PATH, APK path in $1 (default: DefendTaipei-x86_64-debug.apk).
set -euo pipefail

APK="${1:-DefendTaipei-x86_64-debug.apk}"
PKG="tw.lijiabao.defendtaipei"
ACTIVITY="${PKG}/.MainActivity"
OUT_DIR="${SMOKE_OUT:-emulator-smoke}"
RUN_SECS="${SMOKE_SECS:-45}"

mkdir -p "$OUT_DIR"
adb wait-for-device
adb shell 'while [[ -z $(getprop sys.boot_completed) ]]; do sleep 1; done'

echo "==> Installing $APK"
adb install -r -t "$APK"

echo "==> Clearing logcat, launching $ACTIVITY"
adb logcat -c
adb shell am force-stop "$PKG" || true
adb shell am start -n "$ACTIVITY"

echo "==> Waiting ${RUN_SECS}s…"
sleep "$RUN_SECS"

echo "==> Collecting evidence"
adb shell screencap -p >"$OUT_DIR/screenshot.png" || true
adb logcat -d >"$OUT_DIR/logcat.txt"
grep -E 'DTPERF' "$OUT_DIR/logcat.txt" | tee "$OUT_DIR/dtperf.txt" || true

PID="$(adb shell pidof -s "$PKG" 2>/dev/null | tr -d '\r' || true)"
echo "pid=${PID:-none}" | tee "$OUT_DIR/pid.txt"

FAIL=0
if [[ -z "${PID}" ]]; then
  echo "FAIL: process not alive after ${RUN_SECS}s"
  FAIL=1
fi

if ! grep -q 'DTPERF' "$OUT_DIR/dtperf.txt" 2>/dev/null; then
  echo "FAIL: no DTPERF line in logcat"
  FAIL=1
fi

# Crash signatures (ignore historical noise before our launch by scanning full dump —
# we cleared logcat just before start).
if grep -E 'FATAL EXCEPTION|Fatal signal|AM crashed:|Native crash' "$OUT_DIR/logcat.txt" \
  | grep -E "$PKG|defend_taipei|RustStdout|AndroidRuntime" >/dev/null 2>&1; then
  echo "FAIL: crash signature in logcat"
  grep -E 'FATAL EXCEPTION|Fatal signal|AM crashed:|Native crash' "$OUT_DIR/logcat.txt" | head -40
  FAIL=1
fi

if [[ "$FAIL" -ne 0 ]]; then
  echo "Smoke test FAILED (see $OUT_DIR/)"
  exit 1
fi

echo "Smoke test OK"
echo "Sample DTPERF:"
head -5 "$OUT_DIR/dtperf.txt"
exit 0
