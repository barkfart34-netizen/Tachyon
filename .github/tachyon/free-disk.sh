#!/usr/bin/env bash
# The runner's disk fills up (the Rust build, Minecraft's files, the prepared
# resources, the APK): drop the preinstalled toolchains Tachyon's build does
# not use. The newest NDK stays.
set -u
df -h / | tail -1
keep=$(basename "${ANDROID_NDK_LATEST_HOME:-none}")
for ndk in "$ANDROID_HOME"/ndk/*; do
  [ "$(basename "$ndk")" = "$keep" ] || sudo rm -rf "$ndk"
done
sudo rm -rf /usr/share/dotnet /opt/ghc /usr/local/.ghcup /usr/share/swift /usr/local/share/boost \
  /opt/hostedtoolcache/CodeQL /opt/hostedtoolcache/go /opt/hostedtoolcache/PyPy /usr/local/lib/node_modules \
  /usr/local/share/powershell /usr/local/share/chromium /opt/microsoft /opt/google || true
sudo docker image prune --all --force > /dev/null 2>&1 || true
df -h / | tail -1
