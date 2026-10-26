#!/usr/bin/env bash
# Builds the Android application (apps/android) into a signed APK, without Gradle:
# cargo-ndk for the native library, then aapt2, zipalign and apksigner from the SDK.
#
#   packaging/android/build-apk.sh [out.apk]
#
# Needs: cargo-ndk, the NDK (ANDROID_NDK_HOME), the SDK (ANDROID_HOME: build-tools and
# a platform), a JDK. ABIS (default "arm64-v8a") lists the architectures to include.
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

version=$(grep -m1 '^version' "$root/Cargo.toml" | cut -d'"' -f2)
IFS=. read -r major minor patch <<<"${version%%[-+]*}"
code=$((major * 10000 + minor * 100 + patch))

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Native library, laid out as lib/<abi>/libtwitchminer.so.
ndk_args=()
for abi in $abis; do ndk_args+=(-t "$abi"); done
release_flag=()
[ "$profile" = release ] && release_flag=(--release)
(cd "$root" && cargo ndk "${ndk_args[@]}" --platform 26 -o "$work/apk/lib" build "${release_flag[@]}" --locked -p twitchminer-android)

# Manifest and resources.
sed -e "s/VERSION_CODE/$code/" -e "s/VERSION_NAME/$version/" "$here/AndroidManifest.xml" > "$work/AndroidManifest.xml"
"$build_tools/aapt2" compile --dir "$here/res" -o "$work/res.zip"
"$build_tools/aapt2" link -o "$work/base.apk" -I "$ANDROID_JAR" \
    --manifest "$work/AndroidManifest.xml" --min-sdk-version 26 --target-sdk-version 34 \
    "$work/res.zip"
(cd "$work/apk" && zip -qr "$work/base.apk" lib)

"$build_tools/zipalign" -f -p 4 "$work/base.apk" "$work/aligned.apk"
mkdir -p "$(dirname "$out")"
"$build_tools/apksigner" sign \
    --ks "${ANDROID_KEYSTORE:-$here/debug.keystore}" \
    --ks-pass "pass:${ANDROID_KEYSTORE_PASSWORD:-android}" \
    --ks-key-alias "${ANDROID_KEY_ALIAS:-twitchminer}" \
    --out "$out" "$work/aligned.apk"
"$build_tools/apksigner" verify "$out"
echo "$out ($version, versionCode $code, $abis)"
