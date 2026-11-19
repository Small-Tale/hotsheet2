#!/bin/bash
# Build HotSheet.saver (HS2-MZM988): a universal macOS screen saver bundle that hosts the standby page.
#   build.sh [output-dir] [web-dist-dir]
# The web dist directory is `clients/web/dist-saver` (`npm run build:saver`): standby.js and standby.css.
# The bundle's page is Resources/standby/index.html beside them.
set -euo pipefail
root="$(cd "$(dirname "$0")" && pwd)"
out="${1:-$root/build}"
web="${2:-$root/../web/dist-saver}"
bundle="$out/HotSheet.saver"
macos="12.0"

rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources/standby" "$out/obj"
for arch in arm64 x86_64; do
  swiftc -swift-version 5 -parse-as-library -emit-library -Xlinker -bundle \
    -module-name HotSheetSaver -target "$arch-apple-macos$macos" \
    -framework ScreenSaver -framework WebKit -framework AppKit \
    -o "$out/obj/HotSheetSaver-$arch" "$root"/Sources/*.swift
done
lipo -create "$out/obj/HotSheetSaver-arm64" "$out/obj/HotSheetSaver-x86_64" \
  -output "$bundle/Contents/MacOS/HotSheetSaver"
cp "$root/Info.plist" "$bundle/Contents/Info.plist"
test -f "$web/standby.js" -a -f "$web/standby.css" || { echo "build the page first: npm run build:saver (in clients/web)" >&2; exit 1; }
cp "$root/Resources/standby/index.html" "$web/standby.js" "$web/standby.css" "$bundle/Contents/Resources/standby/"
# Ad hoc signature: enough to load locally. Developer ID signing and notarization are a release step.
codesign --force --sign - "$bundle"
echo "$bundle"
