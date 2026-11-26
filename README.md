# Achievement Shelf

A desktop widget for Windows that shows your Steam and GOG Galaxy achievements on a set of shelves in the corner of your screen. You pick which achievements sit on the trophy shelf, and new ones appear as you unlock them.

It reads the achievement data Steam and GOG Galaxy already store on your PC, so there's no account to connect and nothing to log in to.

## Features

- **Trophy shelf:** pick the achievements you're proudest of and arrange them in the order you want.
- **Recent unlocks:** extra rows that fill with your latest achievements and update live while you play.
- **Rarity tiers:** each achievement is coloured by how many players have it: Common, Uncommon, Rare, Epic or Legendary.
- **Details on hover:** the game's banner, name, achievement description, unlock date and rarity.
- **Steam and GOG together** on one shelf.
- **Stays out of the way:** clicks outside the shelf go straight through to your desktop. It can sit behind your windows or stay on top.
- **Updates itself** when a new version is released.

## Install

1. Download `Achievement.Shelf_<version>_x64-setup.exe` from the [latest release](https://github.com/ConnorKnoetze/achievement-shelf/releases/latest).
2. Run it. Windows may show a SmartScreen warning because the installer isn't code-signed. Click **More info**, then **Run anyway**.

The shelf appears in the bottom-right corner of your screen. Everything else is in the tray icon's menu:

- **Choose trophies...** to pick the trophy shelf
- **Settings...** for the number of shelf levels, which display it's on, and its size
- **Keep above other windows** and **Start with Windows**
- **Refresh** and **Quit**

## Requirements

- Windows 10 or 11
- Steam and/or GOG Galaxy installed, signed in, and used on this PC at least once

Steam rarity percentages that aren't stored locally are fetched from Steam's public Web API and cached for a week. Icons and banners are downloaded from the Steam and GOG image servers.

## Development

You need [Node.js](https://nodejs.org/) and the [Rust toolchain](https://rustup.rs/), plus the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for Windows.

```sh
npm install
npm run tauri dev     # run with hot reload
npm run tauri build   # build installers into src-tauri/target/release/bundle
```

Built with [Tauri 2](https://v2.tauri.app/), TypeScript and Vite.

## Releases

Every push to `main` runs [`.github/workflows/release.yml`](.github/workflows/release.yml), which builds signed installers and publishes them as a GitHub Release. The version is `<major>.<minor>.<run number>`, where major and minor come from `src-tauri/tauri.conf.json`.

Installed copies check `releases/latest/download/latest.json` when they start and install newer versions automatically. Update packages are signed with a key stored in the `TAURI_SIGNING_PRIVATE_KEY` repository secret. If that key is lost, installed copies can't update again.
