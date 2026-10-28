# Disk Atlas

A macOS disk usage visualizer that draws your filesystem as an early-20th-century printed world atlas, laid on a wooden table. Area is bytes: folders become continents, nations, provinces and islands, named from their most distinctive word (Npmia, Kingdom of Gradle, Isle of Claude). File counts become cities, small app bundles are free cities, free space is the sea, and system files freeze into polar ice. Elevation is recency, so recently touched data rises into mountains while stale data lies flat, keyed by a "days since last modified" strip printed on the sheet. Hover a region for the note in your hand (name, path, size, age), click to descend, ⌘-click to reveal in Finder. Zooming works like a magnifier: finer region patches and terrain tiles are generated on demand as you go.

A Rust core (`src-tauri/src`) scans the disk, builds the region tree and generates Voronoi-mesh terrain; a TypeScript/WebGL2 frontend (`src`) renders it inside a Tauri app.

Develop with hot reload:

```sh
npm install
npm run tauri dev
```

To use Chrome devtools, run the same engine over HTTP and open `http://localhost:1420` (Vite proxies `/api`). The CLI only has your terminal's permissions, so protected folders like `~/Library/Containers` prompt or stall; for whole-disk scans, launch the installed app with its HTTP mirror instead, since it holds Full Disk Access:

```sh
npm run dev
(cd src-tauri && cargo run --bin atlas -- serve 7878)            # or:
open --env DISK_ATLAS_HTTP=7878 ~/Applications/"Disk Atlas.app"
```

`cargo run --bin atlas -- scan <folder>` prints a quick summary of a scan without any UI.

Build and install:

```sh
npm run tauri build -- --debug        # drop --debug for an optimised build
cp -R "src-tauri/target/debug/bundle/macos/Disk Atlas.app" ~/Applications/
```

The bundle is signed with `bundle.macOS.signingIdentity` in `tauri.conf.json`. Full Disk Access is tied to that identity and the bundle ID, so rebuilds keep the grant; the app must stay unsandboxed. The icon source is `src-tauri/icons/source.svg`; render it to a 1024px PNG and run `npx tauri icon <png>`. Builds aren't notarised, so friends need to right-click → Open once and grant Full Disk Access themselves.
