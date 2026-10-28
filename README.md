# Disk Atlas

A disk usage visualizer for macOS and Windows that draws your filesystem as an old printed world atlas. Folders become continents, nations and islands sized by bytes, files become cities, free space is the sea, and recently modified data rises into mountains.

Hover a region for its path, size and age, click to zoom in, and ⌘-click (Ctrl-click on Windows) to reveal it in Finder or Explorer.

## Download

- **macOS** (Apple Silicon and Intel): [Disk-Atlas.dmg](https://github.com/guillaume-jones/disk-atlas/releases/latest/download/Disk-Atlas.dmg). Drag the app to Applications and open it. macOS will refuse at first, since the app isn't notarised: go to System Settings → Privacy & Security and click Open Anyway. To scan the whole disk, also add the app under Privacy & Security → Full Disk Access.
- **Windows**: [Disk-Atlas-setup.exe](https://github.com/guillaume-jones/disk-atlas/releases/latest/download/Disk-Atlas-setup.exe). If SmartScreen warns, click More info → Run anyway. It installs for the current user, no admin needed.

Reinstalling is the same as installing. All versions are on the [releases page](https://github.com/guillaume-jones/disk-atlas/releases).

## Development

A Rust core (`src-tauri/src`) scans the disk, builds the region tree and generates the terrain; a TypeScript/WebGL2 frontend (`src`) renders it inside a Tauri app.

```sh
npm install
npm run tauri dev
```

`cargo run --example atlas -- scan <folder>` (from `src-tauri`) prints a summary of a scan without any UI.

Build and install locally on macOS:

```sh
npm run tauri build
cp -R "src-tauri/target/release/bundle/macos/Disk Atlas.app" ~/Applications/
```

Local Mac builds are signed with `bundle.macOS.signingIdentity` in `tauri.conf.json`. Full Disk Access is tied to that identity and the bundle ID, so rebuilds keep the grant. On Windows, `npm run tauri build` produces the installer in `src-tauri/target/release/bundle/nsis/`; it needs the [Tauri prerequisites](https://tauri.app/start/prerequisites/).

## Releasing

Bump `version` in `src-tauri/tauri.conf.json`, then push a matching tag:

```sh
git tag v0.1.0 && git push origin v0.1.0
```

GitHub Actions builds a universal macOS DMG and a Windows installer and attaches them to a release for that tag. CI builds are ad-hoc signed, so a downloaded app needs its own Full Disk Access grant.
