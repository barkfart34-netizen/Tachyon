#!/usr/bin/env bash
# Builds Tachyon's APK: the game's library for arm64, the resources prepared
# from Minecraft (TACHYON_RESOURCES, the folder holding horizon-client/26.3)
# as one asset, then Gradle around them.
#
#   TACHYON_RESOURCES=dir android/build-apk.sh
#
#   TACHYON_PROFILE      cargo profile: android (no LTO, quick) or release
#   TACHYON_VERSION_CODE the APK's version code (a newer APK needs a larger one)
#   TACHYON_KEYSTORE, TACHYON_KEYSTORE_PASSWORD, TACHYON_KEY_ALIAS sign it
#     (else Gradle's debug key does)
#
# Needs Rust with the aarch64-linux-android target, the Android SDK
# (ANDROID_HOME) with an NDK (ANDROID_NDK_HOME), a JDK for Gradle, zip and
# python3. The APK lands in android/build/Tachyon.apk.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo=$(dirname "$here")
build="$here/build"
stage="$build/stage"
profile="${TACHYON_PROFILE:-android}"
resources="${TACHYON_RESOURCES:?the prepared resources folder (horizon-client/26.3 inside)}"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/crates/horizon-client/Cargo.toml" | head -n 1)
commit=$(git -C "$repo" rev-parse --short=10 HEAD 2>/dev/null || echo unknown)
code="${TACHYON_VERSION_CODE:-1}"
export TACHYON_BUILD="${TACHYON_BUILD:-$version-$code+$commit}"

# The phone's arm64, or x86_64 for the emulator test (TACHYON_TARGET).
target="${TACHYON_TARGET:-aarch64-linux-android}"
case "$target" in
    aarch64-linux-android) abi=arm64-v8a ;;
    x86_64-linux-android) abi=x86_64 ;;
    *) echo "unknown target $target"; exit 1 ;;
esac
ndk="${ANDROID_NDK_HOME:?ANDROID_NDK_HOME}"
llvm="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin"
api=29
env_target=$(echo "$target" | tr - _)
export "CC_$env_target=$llvm/$target$api-clang"
export "CXX_$env_target=$llvm/$target$api-clang++"
export "AR_$env_target=$llvm/llvm-ar"
export "CARGO_TARGET_$(echo "$env_target" | tr a-z A-Z)_LINKER=$llvm/$target$api-clang"

rm -rf "$stage"
mkdir -p "$stage/jniLibs/$abi" "$stage/assets"

echo "== The game ($TACHYON_BUILD, profile $profile)"
cargo build --manifest-path "$repo/Cargo.toml" -p horizon-client --example tachyon \
    --target "$target" --profile "$profile" \
    --no-default-features --features audio,android
"$llvm/llvm-strip" --strip-debug -o "$stage/jniLibs/$abi/libtachyon.so" \
    "$repo/target/$target/$profile/examples/libtachyon.so"

echo "== The resources"
[ -f "$resources/horizon-client/26.3/horizon-resources.json" ] \
    || { echo "no prepared resources in $resources"; exit 1; }
# Sounds and images are compressed already: stored, they unpack faster.
(cd "$resources" && zip -q -r -X -n .ogg:.png:.jar:.zip "$stage/assets/resources.zip" horizon-client)
# The resources' id: a new one makes the app unpack them again.
python3 -I - "$stage/assets/resources.zip" > "$stage/assets/resources.id" <<'EOF'
import hashlib, sys
h = hashlib.sha256()
with open(sys.argv[1], "rb") as f:
    for chunk in iter(lambda: f.read(1 << 20), b""):
        h.update(chunk)
print(h.hexdigest()[:24])
EOF

echo "== Tachyon's built-in content (tachyon/content)"
# Its own asset and id: a new mod unpacks alone, not Minecraft's resources.
(cd "$repo/tachyon/content" && find . -type f | sort | zip -q -X -D -@ "$stage/assets/content.zip")
python3 -I - "$repo/tachyon/content" > "$stage/assets/content.id" <<'EOF'
import hashlib, pathlib, sys
root = pathlib.Path(sys.argv[1])
h = hashlib.sha256()
for f in sorted(p for p in root.rglob("*") if p.is_file()):
    h.update(f.relative_to(root).as_posix().encode() + b"\0" + f.read_bytes())
print(h.hexdigest()[:24])
EOF
ls -la "$stage/assets"

echo "== The APK"
cd "$here"
echo "sdk.dir=$ANDROID_HOME" > local.properties
# Gradle itself (setup-gradle in CI): the wrapper's jar is not in the repository.
"${GRADLE:-gradle}" --no-daemon -q :app:assembleRelease \
    -PtachyonVersionName="$version" \
    -PtachyonAbi="$abi" \
    -PtachyonVersionCode="$code"
cp app/build/outputs/apk/release/app-release.apk "$build/Tachyon.apk"
ls -la "$build/Tachyon.apk"
