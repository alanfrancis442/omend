#!/bin/sh
# systemd-sleep hook. Install as /usr/lib/systemd/system-sleep/omend.
# After resume, re-arm the EC fan watchdog and rewrite the keyboard zones.
# Use the installed client by absolute path and bound the D-Bus call so a
# stalled bus cannot delay resume. The daemon's own poll also corrects a fan
# mode the firmware reset during sleep.
if [ "$1" = "post" ] && [ -x /usr/bin/omen-cli ]; then
  timeout 10 /usr/bin/omen-cli reapply || true
fi
exit 0
