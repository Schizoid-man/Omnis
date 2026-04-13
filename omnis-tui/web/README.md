# Omnis Desktop Frontend

Next.js frontend for the Omnis desktop shell, styled with shadcn-style UI components.

## Commands

`bun run dev`
Start Next.js development mode on `127.0.0.1:3000`.

`bun run build`
Create a static export in `out/` for Tauri packaging.

`bun run tauri dev`
Run Tauri from this folder using `../src-tauri/tauri.conf.json`.

`npm run tauri:build:linux`
Build Linux desktop bundles (`.AppImage` + `.deb`) on a Linux host.

`npm run tauri:build:dmg`
Build a macOS `.dmg` on a macOS host.

## CI Packaging

GitHub Actions workflow [omnis-tui/.github/workflows/desktop-packages.yml](../.github/workflows/desktop-packages.yml) builds:

- Linux Flatpak bundle artifact (`omnis-desktop-linux-flatpak`)
- macOS DMG artifact (`omnis-desktop-macos-dmg`)

Run it via Actions -> `Desktop Packages` -> `Run workflow`.

## Notes

- Theme is dark-mode only by design.
- Browser dev mode works without crashing when Tauri internals are not present.
- Desktop mode still uses Tauri command invocation.
