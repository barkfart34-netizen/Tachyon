#!/usr/bin/env bash
# Tachyon's emulator test: installs the x86_64 APK, puts smoke-args.txt in the
# app's shared folder (the launcher then plays at once with them), and follows
# the log: the resources unpack, the game starts, a creative world opens and
# the player stays in it. Screenshots, the log and crash lines go to smoke/.
set -u
mkdir -p smoke
PKG=com.tachyon.game
SHARED=/storage/emulated/0/Android/data/$PKG/files

shot() { adb exec-out screencap -p > "smoke/$1.png"; }
alive() { adb shell pidof $PKG > /dev/null; }
log() { adb logcat -d -s Tachyon:*; }
# Waits up to $2 seconds for the log to have $1, a screenshot each minute.
wait_for() {
  local n=0
  for _ in $(seq 1 $(($2 / 5))); do
    log | grep -q "$1" && return 0
    alive || return 1
    n=$((n + 1))
    [ $((n % 12)) -eq 0 ] && shot "wait-$3-$n"
    sleep 5
  done
  return 1
}
finish() {
  shot last
  adb logcat -d > smoke/logcat.txt
  log > smoke/tachyon-log.txt
  echo "== status: $1"
  echo "== Tachyon log (last 80 lines)"; tail -80 smoke/tachyon-log.txt
  echo "== crash lines"; grep -E "FATAL|DEBUG   :|panicked|UnsatisfiedLink|SIGSEGV|SIGABRT" smoke/logcat.txt | head -60
  [ "$1" = ok ]
  exit $?
}

adb shell svc power stayon true
adb shell settings put system screen_off_timeout 1800000
sleep 30
adb install -r android/build/Tachyon.apk || finish "install failed"
# The shared folder exists once the app made it; make it first for the arguments.
adb shell mkdir -p $SHARED
echo "--world Smoke --creative --seed 5" | adb shell "cat > $SHARED/smoke-args.txt"
adb shell ls -la $SHARED
adb logcat -c
adb shell monkey -p $PKG -c android.intent.category.LAUNCHER 1
sleep 20
shot launcher
alive || finish "crashed at start"

wait_for "Tachyon launcher: smoke test" 600 unpack || finish "the resources did not unpack (or smoke-args.txt was not read)"
shot unpacked
wait_for "Tachyon status: title\|Tachyon status: joining\|Tachyon status: world" 600 start || finish "the game did not start"
shot started
wait_for "Tachyon status: world" 900 world || finish "the world did not open"
sleep 30
shot world
alive || finish "crashed in the world"
sleep 60
shot world-later
alive || finish "crashed after a minute in the world"
log | grep "Tachyon status" | tail -3
finish ok
