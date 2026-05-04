# macOS SDK Setup for Docker Builds

The macOS Dockerfile cross-compiles via `osxcross`, which requires the macOS SDK.
Apple's SDK cannot be included in a Docker image — you must extract it yourself from a real Mac.

## One-time extraction (on a Mac)

```bash
# Requires Xcode to be installed from the App Store
xcode-select --install 2>/dev/null || true

SDK_PATH=$(xcrun --show-sdk-path)
SDK_NAME=$(basename "$SDK_PATH")   # e.g. MacOSX14.0.sdk

cd "$(dirname "$SDK_PATH")"
tar -cJf "${SDK_NAME}.tar.xz" "${SDK_NAME}"
echo "Created: $(pwd)/${SDK_NAME}.tar.xz"
```

This produces a file like `MacOSX14.0.sdk.tar.xz` (~120 MB).

## Place the SDK in the build context

Copy the SDK tarball into `omnis-tui/sdk/`:

```
omnis-tui/
  sdk/
    MacOSX14.0.sdk.tar.xz   ← place it here
  Dockerfile.macos
  docker-compose.yml
```

The `sdk/` directory is listed in `.dockerignore` for all other targets — it's only used by `Dockerfile.macos`.

## Build

```bash
# From the omnis-tui/ directory:
docker compose build macos
docker compose run macos
```

Output lands in `omnis-tui/out/macos/Omnis Desktop.zip`.

## Installing on macOS

1. Unzip `Omnis Desktop.zip` — you get `Omnis Desktop.app`
2. Drag it to `/Applications`
3. First launch: right-click → **Open** (Gatekeeper blocks unsigned apps on double-click)

## Converting to a signed DMG (optional)

On a Mac with an Apple Developer account:

```bash
# Sign the app
codesign --deep --force --sign "Developer ID Application: Your Name (TEAMID)" "Omnis Desktop.app"

# Create a DMG
hdiutil create \
  -volname "Omnis Desktop" \
  -srcfolder "Omnis Desktop.app" \
  -ov -format UDZO \
  "Omnis Desktop.dmg"

# Notarize (optional, for Gatekeeper-bypass without right-click)
xcrun notarytool submit "Omnis Desktop.dmg" \
  --apple-id your@email.com \
  --team-id TEAMID \
  --password "@keychain:AC_PASSWORD" \
  --wait
```

## SDK version mapping

| Xcode   | SDK name      | Darwin triple          |
|---------|---------------|------------------------|
| 15.x    | MacOSX14.0    | x86_64-apple-darwin23  |
| 14.x    | MacOSX13.0    | x86_64-apple-darwin22  |
| 13.x    | MacOSX12.0    | x86_64-apple-darwin21  |

If your SDK version differs, update `docker-compose.yml`:

```yaml
macos:
  build:
    args:
      MACOS_SDK_FILE: MacOSX13.0.sdk.tar.xz
      DARWIN_TRIPLE: x86_64-apple-darwin22
```
