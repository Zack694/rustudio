#!/bin/bash
# Compiles the Android Java side (tools/android/java) into assets/android/ImeBridge.dex,
# which src/android/ime.rs builds into the native library (include_bytes!) and
# loads at start with InMemoryDexClassLoader — the APK build itself needs no
# Java toolchain, Gradle or SDK; this is run when the Java changes, and the dex
# is committed.
#
# Needs: a JDK (javac 11+), android.jar of an SDK platform (API 26+) and d8
# (build-tools 30+ or the standalone r8/d8 jar). Point at them with:
#   ANDROID_JAR=/path/to/platforms/android-33/android.jar
#   D8=/path/to/build-tools/34.0.0/d8        (or D8_JAR=/path/to/lib/d8.jar)
# Without those, the script looks under $ANDROID_HOME / $ANDROID_SDK_ROOT.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
src="$here/java"
out="$root/assets/android/ImeBridge.dex"
sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"

if [ -z "${ANDROID_JAR:-}" ]; then
    ANDROID_JAR=$(ls -1 "$sdk"/platforms/android-*/android.jar 2>/dev/null | sort -V | tail -1 || true)
fi
[ -f "${ANDROID_JAR:-}" ] || { echo "build-dex: no android.jar (set ANDROID_JAR)"; exit 1; }
if [ -z "${D8:-}" ] && [ -z "${D8_JAR:-}" ]; then
    D8=$(ls -1 "$sdk"/build-tools/*/d8 2>/dev/null | sort -V | tail -1 || true)
    [ -n "$D8" ] || D8_JAR=$(ls -1 "$sdk"/build-tools/*/lib/d8.jar 2>/dev/null | sort -V | tail -1 || true)
fi
[ -n "${D8:-}" ] || [ -f "${D8_JAR:-}" ] || { echo "build-dex: no d8 (set D8 or D8_JAR)"; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/classes" "$work/dex"
# Java 8 bytecode: what d8 takes for --min-api 26 without desugaring surprises.
javac -encoding UTF-8 -source 8 -target 8 -Xlint:-options -g:none \
    -bootclasspath "$ANDROID_JAR" -d "$work/classes" $(find "$src" -name '*.java')
if [ -n "${D8:-}" ]; then
    "$D8" --release --min-api 26 --lib "$ANDROID_JAR" --output "$work/dex" $(find "$work/classes" -name '*.class')
else
    java -cp "$D8_JAR" com.android.tools.r8.D8 --release --min-api 26 --lib "$ANDROID_JAR" --output "$work/dex" $(find "$work/classes" -name '*.class')
fi
mkdir -p "$(dirname "$out")"
cp "$work/dex/classes.dex" "$out"
echo "build-dex: $(stat -c %s "$out") bytes -> ${out#$root/}"
