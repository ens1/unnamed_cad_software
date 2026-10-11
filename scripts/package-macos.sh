#!/usr/bin/env sh
set -eu
TCAD_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$TCAD_ROOT"
cargo build --workspace --locked
TCAD_DEST="$TCAD_ROOT/dist/TCAD.app"
mkdir -p "$TCAD_ROOT/dist"
TCAD_STAGE=$(mktemp -d "$TCAD_ROOT/dist/.tcad-package.XXXXXX")
TCAD_APP="$TCAD_STAGE/TCAD.app"
TCAD_PREVIOUS="$TCAD_STAGE/previous.app"
cleanup() {
    if { [ -e "$TCAD_PREVIOUS" ] || [ -L "$TCAD_PREVIOUS" ]; } &&
        ! { [ -e "$TCAD_DEST" ] || [ -L "$TCAD_DEST" ]; }; then
        if ! mv "$TCAD_PREVIOUS" "$TCAD_DEST"; then
            printf 'Could not restore previous package; retained at %s\n' "$TCAD_PREVIOUS" >&2
            return 1
        fi
    fi
    rm -rf "$TCAD_STAGE"
}
trap cleanup 0
trap 'exit 1' HUP INT TERM
# Fresh files avoid stale macOS executable-page signatures from in-place copies.
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
plutil -lint "$TCAD_APP/Contents/Info.plist"
# Sign nested code first, then seal the completed local-development bundle.
codesign --force --sign - --timestamp=none --identifier local.tcad.worker \
    "$TCAD_APP/Contents/MacOS/tcad-worker"
codesign --force --sign - --timestamp=none --identifier local.tcad.cli \
    "$TCAD_APP/Contents/MacOS/tcad-cli"
codesign --force --sign - --timestamp=none "$TCAD_APP"
codesign --verify --strict "$TCAD_APP/Contents/MacOS/tcad-worker"
codesign --verify --strict "$TCAD_APP/Contents/MacOS/tcad-cli"
codesign --verify --deep --strict "$TCAD_APP"
# Keep the prior bundle until the replacement is built, signed and verified.
if [ -e "$TCAD_DEST" ] || [ -L "$TCAD_DEST" ]; then
    mv "$TCAD_DEST" "$TCAD_PREVIOUS"
fi
mv "$TCAD_APP" "$TCAD_DEST"
printf 'Packaged and signature-verified %s\n' "$TCAD_DEST"
