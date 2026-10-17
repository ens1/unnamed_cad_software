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
cp target/debug/tcad-git-askpass "$TCAD_APP/Contents/MacOS/tcad-git-askpass"
# Pin the browser sign-in helper independently of TCAD's forge contract. Never
# depend on Finder's PATH or copy an unversioned executable from the build host.
TCAD_GH_VERSION=2.83.2
case "$(uname -m)" in
    arm64)
        TCAD_GH_ARCH=arm64
        TCAD_GH_SHA=ba3e0396ebbc8da17256144ddda503e4e79c8b502166335569f8390d6b75fa8d
        ;;
    x86_64)
        TCAD_GH_ARCH=amd64
        TCAD_GH_SHA=6f1712519ccc768946791dc97da407bf188582345b73fef3d604d050ebf6f614
        ;;
    *) printf 'Unsupported Mac architecture for the GitHub helper\n' >&2; exit 1 ;;
esac
TCAD_GH_NAME="gh_${TCAD_GH_VERSION}_macOS_${TCAD_GH_ARCH}"
TCAD_GH_ARCHIVE="$TCAD_ROOT/target/package-downloads/$TCAD_GH_NAME.zip"
mkdir -p "$(dirname "$TCAD_GH_ARCHIVE")"
if [ ! -f "$TCAD_GH_ARCHIVE" ]; then
    curl --fail --location --silent --show-error --retry 3 --connect-timeout 20 --max-time 180 \
        "https://github.com/cli/cli/releases/download/v$TCAD_GH_VERSION/$TCAD_GH_NAME.zip" \
        --output "$TCAD_STAGE/github-cli.zip"
    printf '%s  %s\n' "$TCAD_GH_SHA" "$TCAD_STAGE/github-cli.zip" | shasum -a 256 --check
    mv "$TCAD_STAGE/github-cli.zip" "$TCAD_GH_ARCHIVE"
fi
printf '%s  %s\n' "$TCAD_GH_SHA" "$TCAD_GH_ARCHIVE" | shasum -a 256 --check
unzip -p "$TCAD_GH_ARCHIVE" "$TCAD_GH_NAME/bin/gh" > "$TCAD_APP/Contents/MacOS/tcad-gh"
chmod 755 "$TCAD_APP/Contents/MacOS/tcad-gh"
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
codesign --force --sign - --timestamp=none --identifier local.tcad.github-helper \
    "$TCAD_APP/Contents/MacOS/tcad-gh"
codesign --force --sign - --timestamp=none --identifier local.tcad.git-askpass \
    "$TCAD_APP/Contents/MacOS/tcad-git-askpass"
codesign --force --sign - --timestamp=none "$TCAD_APP"
codesign --verify --strict "$TCAD_APP/Contents/MacOS/tcad-worker"
codesign --verify --strict "$TCAD_APP/Contents/MacOS/tcad-cli"
codesign --verify --strict "$TCAD_APP/Contents/MacOS/tcad-gh"
codesign --verify --strict "$TCAD_APP/Contents/MacOS/tcad-git-askpass"
codesign --verify --deep --strict "$TCAD_APP"
# Keep the prior bundle until the replacement is built, signed and verified.
if [ -e "$TCAD_DEST" ] || [ -L "$TCAD_DEST" ]; then
    mv "$TCAD_DEST" "$TCAD_PREVIOUS"
fi
mv "$TCAD_APP" "$TCAD_DEST"
printf 'Packaged and signature-verified %s\n' "$TCAD_DEST"
