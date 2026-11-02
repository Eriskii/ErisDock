#!/bin/sh
# Builds ErisDock, puts it in ~/Applications in place of any older one, and starts it.
set -eu
cd "$(dirname "$0")/.."
scripts/build.sh
pkill -x ErisDock 2>/dev/null || true
while pgrep -xq ErisDock; do sleep 0.1; done
mkdir -p ~/Applications
rm -rf ~/Applications/ErisDock.app
cp -R build/ErisDock.app ~/Applications/
open ~/Applications/ErisDock.app
