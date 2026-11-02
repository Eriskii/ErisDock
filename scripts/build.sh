#!/bin/sh
# Builds build/ErisDock.app from a release build, signed ad hoc unless ERISDOCK_SIGN names a
# signing identity. Accessibility access belongs to the signature, so an ad hoc build has to be
# allowed again after each rebuild; a lasting identity keeps it.
set -eu
cd "$(dirname "$0")/.."
cargo build --release
app=build/ErisDock.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp target/release/erisdock "$app/Contents/MacOS/ErisDock"
cp Resources/Info.plist "$app/Contents/Info.plist"
codesign --force --sign "${ERISDOCK_SIGN:--}" "$app"
echo "$app"
