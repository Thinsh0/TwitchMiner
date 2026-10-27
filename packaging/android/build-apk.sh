#!/usr/bin/env bash
# Builds the Android application into a signed APK: cargo-ndk for the native library
# (apps/android), Gradle for the Java glue and Firebase (build.gradle.kts), then
# apksigner.
#
#   packaging/android/build-apk.sh [out.apk]
#
# Needs: cargo-ndk, the NDK (ANDROID_NDK_HOME), the SDK (ANDROID_HOME: build-tools and
# a platform), JDK 17+. ABIS (default "arm64-v8a") lists the architectures to include.
# Notifications pushed by the server: GOOGLE_SERVICES_JSON holds the content of the
# Firebase project's google-services.json; without it the APK is built without them.
# Signing: ANDROID_KEYSTORE / ANDROID_KEYSTORE_PASSWORD / ANDROID_KEY_ALIAS when set,
# otherwise the public test key next to this script (anyone can sign with it: fine for
# trying the app, not for distributing it widely).
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out=${1:-$root/target/TwitchMiner.apk}
abis=${ABIS:-arm64-v8a}
profile=${PROFILE:-release}

: "${ANDROID_HOME:?ANDROID_HOME must point to the Android SDK}"
build_tools=$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)
# A released platform: CI images also carry previews (reported as "DEV").
platform=$ANDROID_HOME/platforms/${ANDROID_PLATFORM:-android-35}
[ -d "$platform" ] || platform=$(ls -d "$ANDROID_HOME"/platforms/android-[0-9]* | sort -V | tail -1)
export ANDROID_JAR=${ANDROID_JAR:-$platform/android.jar}
compile_sdk=${platform##*android-}

version=$(grep -m1 '^version' "$root/Cargo.toml" | cut -d'"' -f2)
IFS=. read -r major minor patch <<<"${version%%[-+]*}"
code=$((major * 10000 + minor * 100 + patch))

# Native library, laid out as <abi>/libtwitchminer.so for Gradle.
jni=$root/target/android-jni
rm -rf "$jni"
ndk_args=()
for abi in $abis; do ndk_args+=(-t "$abi"); done
release_flag=()
[ "$profile" = release ] && release_flag=(--release)
(cd "$root" && cargo ndk "${ndk_args[@]}" --platform 26 -o "$jni" build "${release_flag[@]}" --locked -p twitchminer-android)

# Firebase configuration: only for this build, never kept in the tree.
firebase=no
rm -f "$here/google-services.json"
if [ -n "${GOOGLE_SERVICES_JSON:-}" ]; then
    printf '%s' "$GOOGLE_SERVICES_JSON" > "$here/google-services.json"
    trap 'rm -f "$here/google-services.json"' EXIT
    firebase=yes
fi

(cd "$here" && ./gradlew --no-daemon --console=plain -q assembleRelease \
    -Ptm.versionCode="$code" -Ptm.versionName="$version" -Ptm.compileSdk="$compile_sdk" -Ptm.abis="$abis")
unsigned=$root/target/android/outputs/apk/release/twitchminer-release-unsigned.apk

# Gradle already aligns the archive.
mkdir -p "$(dirname "$out")"
"$build_tools/apksigner" sign \
    --ks "${ANDROID_KEYSTORE:-$here/debug.keystore}" \
    --ks-pass "pass:${ANDROID_KEYSTORE_PASSWORD:-android}" \
    --ks-key-alias "${ANDROID_KEY_ALIAS:-twitchminer}" \
    --out "$out" "$unsigned"
"$build_tools/apksigner" verify "$out"
rm -f "$out.idsig"
echo "$out ($version, versionCode $code, $abis, Firebase: $firebase)"
