#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"

APP_NAME="ClipVault"
SDK="/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk"
BUILD_DIR="build"
APP_BUNDLE="$BUILD_DIR/$APP_NAME.app"

rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS" "$APP_BUNDLE/Contents/Resources"

echo "Compiling..."
swiftc \
  Sources/ClipVault/ClipVaultMain.swift \
  Sources/ClipVault/AppDelegate.swift \
  Sources/ClipVault/Core/*.swift \
  Sources/ClipVault/UI/*.swift \
  -o "$APP_BUNDLE/Contents/MacOS/$APP_NAME" \
  -O \
  -parse-as-library \
  -swift-version 5 \
  -strict-concurrency=minimal \
  -sdk "$SDK" \
  -target arm64-apple-macosx13.0 \
  -framework AppKit \
  -framework SwiftUI \
  -framework Carbon \
  -framework ServiceManagement \
  -framework CryptoKit \
  -lsqlite3

cp Resources/Info.plist "$APP_BUNDLE/Contents/Info.plist"

echo "Code signing (ad-hoc)..."
codesign --force --deep --sign - "$APP_BUNDLE"

echo "Built $APP_BUNDLE"
