#!/usr/bin/env bash
# Tachyon's emulator test: installs the x86_64 APK, starts it with the intent
# extra tachyon.smoke (the launcher then plays at once with it), and follows
# the log: the resources unpack, the game starts, a creative world opens and
# the player stays in it. Screenshots, the log and crash lines go to smoke/.
set -u
mkdir -p smoke
PKG=com.tachyon.game
LIVE=

shot() { adb exec-out screencap -p > "smoke/$1.png"; }
alive() { adb shell pidof $PKG > /dev/null; }
log() { cat smoke/tachyon-live.txt; }
# Waits up to $2 seconds for the log to have $1, a screenshot each minute.
wait_for() {
  local n=0
  for _ in $(seq 1 $(($2 / 5))); do
    log | grep -q "$1" && return 0
    alive || return 1
    adb shell am broadcast -a android.intent.action.CLOSE_SYSTEM_DIALOGS > /dev/null 2>&1
    n=$((n + 1))
    [ $((n % 12)) -eq 0 ] && shot "wait-$3-$n"
    sleep 5
  done
  return 1
}
finish() {
  shot last
  adb logcat -d > smoke/logcat.txt
  kill "$LIVE" 2> /dev/null
  echo "== status: $1"
  echo "== Tachyon log (last 80 lines)"; tail -80 smoke/tachyon-live.txt
  echo "== crash lines"; grep -E "FATAL|DEBUG   :|panicked|UnsatisfiedLink|SIGSEGV|SIGABRT" smoke/logcat.txt | head -60
  [ "$1" = ok ]
  exit $?
}

adb shell svc power stayon true
adb shell settings put system screen_off_timeout 1800000
sleep 30
adb install -r android/build/Tachyon.apk || finish "install failed"
# A large log buffer, and the app's lines kept as they come.
adb logcat -G 16M
adb logcat -c
adb logcat -s Tachyon:* > smoke/tachyon-live.txt &
LIVE=$!
adb shell am start -n $PKG/.TachyonActivity --es tachyon.smoke "--world,Smoke,--creative,--seed,5"
sleep 20
shot launcher
alive || finish "crashed at start"

wait_for "Tachyon launcher: smoke test" 600 unpack || finish "the resources did not unpack (or the smoke extra was not read)"
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

# Bedrock's touch controls (the default): drag the joystick up for a few
# seconds and the player walks; the status lines (every 10 s) show it.
where=$(log | grep -o "Tachyon touch: [a-z]*, move at [0-9]*,[0-9]*" | tail -1 | grep -o "[0-9]*,[0-9]*$")
[ -n "$where" ] || finish "no Bedrock touch controls in the world"
mx=${where%,*}
my=${where#*,}
before=$(log | grep "Tachyon status: world" | tail -1 | grep -o "player at .*")
echo "joystick at $mx,$my; before: $before"
adb shell input swipe "$mx" "$my" "$mx" "$((my - 120))" 6000
shot joystick
sleep 22
after=$(log | grep "Tachyon status: world" | tail -1 | grep -o "player at .*")
echo "after: $after"
[ -n "$before" ] && [ "$before" != "$after" ] || finish "the joystick did not move the player"
alive || finish "crashed after the joystick"
log | grep "Tachyon status" | tail -3
finish ok
