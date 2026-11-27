# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(Rendered in a Tauri v2 WebView2 window on Windows; the design language is web, not native.)

## Users
One PC gamer who plays on Steam, GOG Galaxy, Ubisoft Connect or the EA app, and perhaps retro games through RetroAchievements or games on Xbox. They keep Achievement Shelf on their own desktop all the time, either behind their windows or pinned on top. It's a trophy case for the achievements they're proudest of, plus their recent unlocks.

## Product Purpose
It turns achievements, which normally sit buried in launcher menus, into a collection that's always on display. A glance at the shelf should make the user feel **pride in a collection**: the achievements are prized objects, and rare ones should feel earned.

## Positioning
It reads the user's local Steam, GOG Galaxy, Ubisoft Connect and EA app achievement data with no account or web login and shows achievements from all of them on one shelf. Online-only sources such as RetroAchievements and Xbox are optional extras that the user signs in to in Settings; the shelf never needs a sign-in to work. The user chooses which ones sit on the trophy shelf, and new unlocks appear by themselves as they happen.

## Operating Context
- Always visible over the user's own wallpaper, which can be anything, light or dark.
- The window is transparent and frameless, and is anchored to the bottom-right of the work area. Clicks outside the shelf pass through to the desktop.
- It's set up from the tray menu: an achievement picker for the trophy shelf, and a settings window for the number of levels and on-top/on-bottom.
- Hovering an achievement shows a tooltip with the game banner, game name, title, description, unlock date and rarity.

## Capabilities and Constraints
- There are two kinds of rows: trophy rows (at least 1, picked by the user) and recent-unlock rows (0 or more), up to 10 levels in total and 6 achievements per row.
- Rarity is the global unlock percentage, sorted into tiers: Common ≥40%, Uncommon ≥15%, Rare ≥5%, Epic ≥1%, Legendary below that. Some achievements have no rarity. When every achievement a game defines is unlocked, the last one unlocked also gets a Platinum label, alongside its rarity tier.
- Achievement icons are square game-supplied images (usually 64px). Some are missing and fall back to a 🏆.
- The window size comes from `SHELF_WIDTH` (520) and `ROW_HEIGHT` (112) in `src-tauri/src/lib.rs`, so changing either means a matching Rust change.
- `backdrop-filter` can't blur the desktop through the transparent window.
- New unlocks animate in while the shelf is running.
- The widget is always on screen, so idle animation has to stay cheap.

## Brand Commitments
- It must still read as **shelves holding achievements**, not a list or a grid panel.
- Keep the rarity tier colours used on tiles and labels: common `#c9c2b6`, uncommon `#7fd38a`, rare `#6cb4f5`, epic `#c79bff`, legendary `#ffb347`.
- Name: Achievement Shelf.

## Evidence on Hand
- The user's real achievement icons and game banners come from the launchers at runtime. There are no bundled sample assets.
- App icon: `src-tauri/app-icon.svg`.

## Product Principles
1. The achievements are the stars: the shelf frames them and never competes with them.
2. Rarity should be visible at a glance, and rare means visibly earned.
3. It lives on someone's desktop all day, so it has to look good over any wallpaper and never get tiring or noisy.
4. Details come on demand (hover), and the resting state stays clean.
