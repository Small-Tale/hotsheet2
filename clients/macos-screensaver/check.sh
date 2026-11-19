#!/bin/bash
# Verify the screen saver bundle and its hosting (HS2-MZM988): bundle structure, the principal class
# loads from the built bundle, and the saver view renders the page, takes snapshots, and resizes.
#   check.sh [work-dir] [web-dist-dir]
set -euo pipefail
root="$(cd "$(dirname "$0")" && pwd)"
work="${1:-$(mktemp -d)}"
web="${2:-$root/../web/dist-saver}"
bundle="$("$root/build.sh" "$work/bundle" "$web" | tail -1)"

test -x "$bundle/Contents/MacOS/HotSheetSaver"
test -f "$bundle/Contents/Resources/standby/index.html"
plutil -lint "$bundle/Contents/Info.plist" >/dev/null
test "$(plutil -extract NSPrincipalClass raw "$bundle/Contents/Info.plist")" = HotSheetSaverView
test "$(lipo -archs "$bundle/Contents/MacOS/HotSheetSaver" | tr ' ' '\n' | sort | tr '\n' ' ')" = "arm64 x86_64 "
codesign --verify --strict "$bundle"

# The principal class is found by the name the engine reads from Info.plist.
cat > "$work/load.swift" <<'SWIFT'
import AppKit
let bundle = Bundle(path: CommandLine.arguments[1])!
guard bundle.load(), let principal = bundle.principalClass else { print("FAIL: bundle did not load"); exit(1) }
guard NSStringFromClass(principal) == "HotSheetSaverView" else { print("FAIL: principal class \(principal)"); exit(1) }
print("principal:\(NSStringFromClass(principal))")
SWIFT
swiftc -o "$work/load" "$work/load.swift"
"$work/load" "$bundle"

# Host the view with the real page and exercise it. The harness compiles the sources directly so it can
# point the snapshot lookup at a temporary home; the page comes from the built bundle's resources.
mkdir -p "$work/home"
node --experimental-strip-types --no-warnings "$root/../web/scripts/write-standby-saver-fixture.mts" "$work/snapshot.json"
swiftc -swift-version 5 -o "$work/harness" \
  -framework AppKit -framework WebKit -framework ScreenSaver \
  "$root/Sources/HotSheetSaverView.swift" "$root/Harness/main.swift" 2>&1 | grep -v "^$" || true
"$work/harness" "$bundle/Contents/Resources/standby" "$work/home" "$work/snapshot.json" ${CAPTURE_PREFIX:+"$CAPTURE_PREFIX"}
