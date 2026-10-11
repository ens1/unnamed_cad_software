#!/usr/bin/env sh
set -eu
TCAD_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$TCAD_ROOT"
cargo build --workspace --locked
TCAD_APP="$TCAD_ROOT/dist/TCAD.app"
mkdir -p "$TCAD_APP/Contents/MacOS" "$TCAD_APP/Contents/Resources"
cp target/debug/tcad-desktop "$TCAD_APP/Contents/MacOS/TCAD"
cp target/debug/tcad-worker "$TCAD_APP/Contents/MacOS/tcad-worker"
# Keep executable names distinct on case-insensitive macOS filesystems.
cp target/debug/tcad "$TCAD_APP/Contents/MacOS/tcad-cli"
cp LICENSE THIRD_PARTY_NOTICES.md "$TCAD_APP/Contents/Resources/"
cp -R third-party-licenses "$TCAD_APP/Contents/Resources/"
cat > "$TCAD_APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>TCAD</string>
<key>CFBundleIdentifier</key><string>local.tcad.desktop</string>
<key>CFBundleName</key><string>TCAD</string>
<key>CFBundleDisplayName</key><string>TCAD</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>NSHighResolutionCapable</key><true/>
<key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
</dict></plist>
PLIST
printf 'Packaged %s\n' "$TCAD_APP"
