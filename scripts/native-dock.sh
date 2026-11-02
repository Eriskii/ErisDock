#!/bin/sh
# ErisDock takes the native Dock's place at the bottom of the screen. `hide` keeps the native
# Dock hidden and slow to come back: the pointer has to rest on the edge for 60000 seconds
# (1000 minutes) before it shows. `restore` puts its usual delay back.
set -eu
case "${1:-}" in
hide)
	defaults write com.apple.dock autohide -bool true
	defaults write com.apple.dock autohide-delay -float 60000
	;;
restore)
	defaults delete com.apple.dock autohide-delay 2>/dev/null || true
	;;
*)
	echo "usage: $0 hide|restore" >&2
	exit 64
	;;
esac
killall Dock
