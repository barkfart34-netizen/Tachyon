#!/usr/bin/env bash
# Prepares the game's resources the way a player's Horizon launcher would,
# from Minecraft downloaded from Mojang: horizon-prepare (with Java 25) and the
# sounds. They land in $1/horizon-client/<version>.
set -euo pipefail
out=$1
repo=$(cd "$(dirname "$0")/../.." && pwd)
mc="$RUNNER_TEMP/minecraft"
python3 -I "$repo/.github/tachyon/fetch-minecraft.py" "$mc"
cargo build --release -p horizon-prepare --manifest-path "$repo/Cargo.toml"
prepare="$repo/target/release/horizon-prepare"
export JAVA_HOME="${JAVA_HOME_25_X64:-$JAVA_HOME}"
"$prepare" --prepare --server-jar "$mc/server.jar" --minecraft-dir "$mc" --root "$repo"
"$prepare" sounds --minecraft-dir "$mc" --root "$repo"
mkdir -p "$out/horizon-client"
rm -rf "$out/horizon-client/26.3"
mv "$repo/.local/horizon-client/26.3" "$out/horizon-client/26.3"
du -sh "$out/horizon-client/26.3"
# Mojang's files and the imports stay out of the cache and the APK.
rm -rf "$mc" "$repo/.local"
