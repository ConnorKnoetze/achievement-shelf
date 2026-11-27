# Achievement Shelf

A desktop widget for Windows that shows your Steam, GOG Galaxy, Ubisoft Connect, EA app, RetroAchievements and Xbox achievements on a set of shelves in the corner of your screen. You pick which achievements sit on the trophy shelf, and new ones appear as you unlock them.

For the PC launchers it reads the achievement data they already store on your PC, so there's no account to connect and nothing to log in to. RetroAchievements, for retro games played in emulators, is optional and needs your username and Web API key. Xbox is optional too, and you sign in with your Microsoft account in your browser.

## Features

- **Trophy shelf:** pick the achievements you're proudest of and arrange them in the order you want.
- **Recent unlocks:** extra rows that fill with your latest achievements and update live while you play.
- **Rarity tiers:** each achievement is coloured by how many players have it: Common, Uncommon, Rare, Epic or Legendary.
- **Platinum:** when you unlock every achievement in a game, the one that completed it gets a Platinum label next to its rarity.
- **Details on hover:** the game's banner, name, achievement description, unlock date and rarity.
- **Steam, GOG, Ubisoft, EA, RetroAchievements and Xbox together** on one shelf.
- **Stays out of the way:** clicks outside the shelf go straight through to your desktop. It can sit behind your windows or stay on top.
- **Updates itself** when a new version is released.

## Install

1. Download `Achievement.Shelf_<version>_x64-setup.exe` from the [latest release](https://github.com/ConnorKnoetze/achievement-shelf/releases/latest).
2. Run it. Windows may show a SmartScreen warning because the installer isn't code-signed. Click **More info**, then **Run anyway**.

The shelf appears in the bottom-right corner of your screen. Everything else is in the tray icon's menu:

- **Choose trophies...** to pick the trophy shelf
- **Settings...** for the number of shelf levels, which display it's on, its size, and signing in to RetroAchievements or Xbox
- **Keep above other windows** and **Start with Windows**
- **Refresh** and **Quit**

## Requirements

- Windows 10 or 11
- At least one of Steam, GOG Galaxy, Ubisoft Connect or the EA app installed, signed in, and used on this PC at least once, or a RetroAchievements or Xbox account

Steam rarity percentages that aren't stored locally are fetched from Steam's public Web API and cached for a week. Icons and banners are downloaded from the Steam, GOG, Ubisoft and EA image servers; Ubisoft achievement icons come from the launcher's own files.

EA achievements are read from the EA app's log, which only covers recent sessions: a game shows up once you've played it since the log was last cleared. Achievement Shelf keeps everything it has read, so unlocks stay on the shelf after the log rotates.

RetroAchievements unlocks and rarity come from the retroachievements.org Web API. Your Web API key is kept in Windows Credential Manager. The shelf checks for new unlocks every few seconds while an emulator that supports RetroAchievements is running, every few minutes otherwise, and re-reads your whole library every few hours or when you choose **Refresh**.

Xbox unlocks and rarity come from Xbox Live. Signing in happens on Microsoft's own page in your browser, so Achievement Shelf never sees your password. It keeps only the sign-in token, in Windows Credential Manager. The shelf checks your Xbox games for new unlocks once a minute. Xbox 360 games aren't shown yet.

## Development

You need [Node.js](https://nodejs.org/) and the [Rust toolchain](https://rustup.rs/), plus the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for Windows.

```sh
npm install
npm run tauri dev     # run with hot reload
npm run tauri build   # build installers into src-tauri/target/release/bundle
```

Xbox sign-in runs as Achievement Shelf's Azure app registration, whose client ID is in `src-tauri/src/xbox.rs`. To use another one (a public client for personal Microsoft accounts with the redirect URI `http://localhost` under **Mobile and desktop applications**), set its client ID in the `XBOX_CLIENT_ID` environment variable when building.

Built with [Tauri 2](https://v2.tauri.app/), TypeScript and Vite.

## Releases

Day-to-day work goes on the `dev` branch, which doesn't build anything. To ship, merge `dev` into `main`.

Every push to `main` runs [`.github/workflows/release.yml`](.github/workflows/release.yml), which builds signed installers and publishes them as a GitHub Release. The version is `<major>.<minor>.<run number>`, where major and minor come from `src-tauri/tauri.conf.json`. The release description is the `## Release notes` section of the merged PR's description (see [AGENTS.md](AGENTS.md)).

Installed copies check `releases/latest/download/latest.json` when they start and install newer versions automatically. Update packages are signed with a key stored in the `TAURI_SIGNING_PRIVATE_KEY` repository secret. If that key is lost, installed copies can't update again.
