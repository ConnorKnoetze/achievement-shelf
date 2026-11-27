---
name: Achievement Shelf
description: A desktop display rail of graded achievement slabs, set up from a record shop at night.
colors:
  card-stock: "#f4f2ec"
  ink: "#16181b"
  ink-muted: "#565b62"
  rail-top: "#34383e"
  rail-face: "#17191c"
  rail-base: "#0a0b0d"
  insert: "#0d1014"
  brass-light: "#e2c07a"
  brass-dark: "#8a642a"
  tier-common: "#c9c2b6"
  tier-uncommon: "#7fd38a"
  tier-rare: "#6cb4f5"
  tier-epic: "#c79bff"
  tier-legendary: "#ffb347"
  window-ground: "#17111c"
  window-plate: "#0d0910"
  window-panel: "#211829"
  window-panel-hover: "#2a2034"
  window-sunk: "#120d16"
  window-line: "#33263d"
  window-line-strong: "#45364f"
  window-line-hover: "#65536f"
  window-text: "#efe8f2"
  window-muted: "#a596ad"
  divider: "#d6c8de"
  on-divider: "#1d1422"
  sticker: "#ffe14d"
  on-sticker: "#1d1422"
  window-ground-light: "#efe9f1"
  window-panel-light: "#f9f6fa"
  window-sunk-light: "#fdfbfd"
  window-line-light: "#ddd2e2"
  window-line-strong-light: "#c3b4cb"
  window-text-light: "#211829"
  window-muted-light: "#625469"
  divider-light: "#c7b2d4"
  sticker-light: "#e0b800"
typography:
  grade-tier:
    fontFamily: "Barlow Condensed, Segoe UI, system-ui, sans-serif"
    fontSize: "8.5px"
    fontWeight: 700
    lineHeight: 1
    letterSpacing: "0.05em"
  grade-pct:
    fontFamily: "Barlow Condensed, Segoe UI, system-ui, sans-serif"
    fontSize: "10px"
    fontWeight: 600
    lineHeight: 1
  title:
    fontFamily: "Segoe UI, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 600
    lineHeight: 1.35
  body:
    fontFamily: "Segoe UI, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.35
  meta:
    fontFamily: "Segoe UI, system-ui, sans-serif"
    fontSize: "11px"
    fontWeight: 400
    lineHeight: 1.35
  window-display:
    fontFamily: "Barlow Condensed, Segoe UI, system-ui, sans-serif"
    fontSize: "24px"
    fontWeight: 700
    lineHeight: 1.1
    letterSpacing: "0.03em"
  window-headline:
    fontFamily: "Barlow Condensed, Segoe UI, system-ui, sans-serif"
    fontSize: "18px"
    fontWeight: 700
    lineHeight: 1.1
    letterSpacing: "0.03em"
  window-figure:
    fontFamily: "Barlow Condensed, Segoe UI, system-ui, sans-serif"
    fontSize: "16px"
    fontWeight: 700
    letterSpacing: "0.01em"
  window-body:
    fontFamily: "Segoe UI, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.4
  window-label:
    fontFamily: "Segoe UI, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 600
    lineHeight: 1.4
  window-hint:
    fontFamily: "Segoe UI, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.4
rounded:
  label: "2px"
  rail: "3px"
  slab: "5px"
  card: "6px"
  sleeve: "3px"
  divider-tab: "6px"
  tile: "8px"
  pill: "999px"
  round: "50%"
spacing:
  slab-pad: "4px"
  slab-gap: "14px"
  rail-gap: "10px"
  window-gutter: "24px"
  control-height: "32px"
  bin-gap: "16px"
  tile-gap: "10px"
components:
  slab:
    width: "62px"
    height: "88px"
    padding: "{spacing.slab-pad}"
    rounded: "{rounded.slab}"
  slab-insert:
    backgroundColor: "{colors.insert}"
    size: "54px"
    rounded: "{rounded.label}"
  grade-pct:
    backgroundColor: "{colors.card-stock}"
    textColor: "{colors.ink}"
    typography: "{typography.grade-pct}"
  rail:
    backgroundColor: "{colors.rail-face}"
    height: "12px"
    rounded: "{rounded.rail}"
  certificate:
    backgroundColor: "{colors.rail-face}"
    textColor: "#eef0f2"
    rounded: "{rounded.card}"
    padding: "12px"
  placard:
    backgroundColor: "{colors.card-stock}"
    textColor: "{colors.ink}"
    rounded: "{rounded.rail}"
    padding: "6px 12px"
  window-button:
    textColor: "{colors.window-text}"
    typography: "{typography.window-label}"
    rounded: "{rounded.pill}"
    height: "{spacing.control-height}"
    padding: "0 15px"
  window-button-hover:
    backgroundColor: "{colors.window-panel-hover}"
  window-segment-active:
    backgroundColor: "{colors.divider}"
    textColor: "{colors.on-divider}"
    rounded: "{rounded.pill}"
    height: "26px"
    padding: "0 13px"
  window-chip:
    textColor: "{colors.window-muted}"
    typography: "{typography.window-label}"
    rounded: "{rounded.pill}"
    height: "28px"
    padding: "0 10px 0 8px"
  window-chip-active:
    backgroundColor: "{colors.divider}"
    textColor: "{colors.on-divider}"
  window-input:
    backgroundColor: "{colors.window-sunk}"
    textColor: "{colors.window-text}"
    rounded: "{rounded.pill}"
    height: "{spacing.control-height}"
    padding: "0 14px"
  divider-card:
    backgroundColor: "{colors.divider}"
    textColor: "{colors.on-divider}"
    typography: "{typography.window-headline}"
    rounded: "{rounded.divider-tab}"
    padding: "6px 14px 4px"
  divider-tab-current:
    backgroundColor: "{colors.divider}"
    textColor: "{colors.on-divider}"
    rounded: "{rounded.divider-tab}"
    width: "38px"
  sticker:
    backgroundColor: "{colors.sticker}"
    textColor: "{colors.on-sticker}"
    rounded: "{rounded.round}"
    size: "48px"
  achievement-tile:
    backgroundColor: "{colors.window-panel}"
    textColor: "{colors.window-text}"
    rounded: "{rounded.tile}"
    padding: "10px 12px"
  settings-group:
    backgroundColor: "{colors.window-panel}"
    rounded: "{rounded.tile}"
    padding: "12px 14px"
---

# Design System: Achievement Shelf

## Overview

**Creative North Star: "The Grading Rail"**

Every achievement is a graded collectible. It's sealed in a clear acrylic slab with a printed grade label, and stands in a matte black display rail fixed to the bottom-right of the user's desktop. The user's own wallpaper is the ground, seen through the acrylic. The system is made of three real materials (clear acrylic, black anodised rail, off-white card stock) plus the printed rarity colour. Nothing glows and nothing is decoration. The grade label is the one place colour appears, so rarity is the first thing the eye reads.

The widget is on screen all day over unknown wallpaper, so every surface has to hold up over both light and dark backgrounds, and it stays still unless the user touches it.

**Key Characteristics:**
- Clear acrylic slabs, 62×88, each with a two-tone grade label and a dark icon insert.
- Matte black rails with a slot channel; the slab's foot sits down in the slot.
- Off-white card stock for every printed surface: grade labels and placards. The tooltip certificate is cut from the rail's black anodised stock; its grade is a small foil chip with the figures printed straight on the black.
- On the shelf, colour comes only from the five rarity tiers.
- Motion only happens when the user acts or something changes: lift on hover, a drop into the slot for new unlocks.

### Setup windows: "Crate Digging"

The trophy picker and settings windows (`picker.html`, `settings.html`, sharing `src/window.css`) are the record shop behind the shelf, lit at night. You flip through your games like a record-shop bin: cover art leads, lilac-grey divider cards jump you through the alphabet, and a round lemon price sticker says "this one has trophies up". They open from the tray now and then, so they can carry their own palette; the shelf's materials (rail, acrylic, printed grade, card-stock placard) appear inside them drawn exactly as on the shelf.

The windows are dark by default, an aubergine ground with a near-black plum plate, and have a light mode that is the same shop by daylight: pale lilac ground, plum ink, a deeper lemon. A drawing of the shelf (a `.case`: the picker's shelf plate, the settings' level diagram) keeps the dark palette in both modes, because the shelf itself stays dark.

**Key Characteristics (setup windows):**
- Cover sleeves in a capped-width grid, filed under a divider-card heading per letter, with an A to Z divider rail down the left of the bin.
- Lilac-grey divider card for every selected or active state: the chosen segment, a pressed chip, the current letter, a checked switch, an armed reset.
- A round lemon sticker that means "on the trophy shelf" and nothing else.
- Pill controls, divider tabs rounded on their outer edge, 8px tiles and groups.
- Barlow Condensed caps for headings, divider letters and figures; Segoe UI for everything read in sentences.

## Colors

The shelf's palette is neutral material (acrylic, black rail, card stock) plus five rarity colours. The setup windows add a plum night-shop palette with two signal colours, each with one job.

### Primary
- **Rarity tiers** (common, uncommon, rare, epic, legendary): printed only as the tier block of a grade label, on slabs and on the certificate. They're shared with the picker's rarity labels and filter swatches and are a brand commitment.

### Secondary (setup windows)
- **Lilac-Grey Divider** (`divider`; `divider-light` by daylight): the divider card. Selection and active state in the windows, the focus ring, the filled part of the size slider, and the drawn record on a sleeve with no cover. Text on it is **Divider Ink** (`on-divider`).

### Tertiary (setup windows)
- **Lemon Sticker** (`sticker`; `sticker-light` by daylight): the price sticker and the edge and faint wash of an achievement tile that is on the trophy shelf. Text on it is **Divider Ink** (`on-sticker`).

### Neutral
- **Card Stock**: the ground of every printed surface (grade percent, placard, and the windows' error slip and slider grip).
- **Ink / Ink Muted**: text on card stock. Muted is for meta lines (game name, date, "Ungraded").
- **Rail Top / Face / Base**: the rail's lit top edge, its front face, and its shadowed foot.
- **Insert**: the dark well behind each icon, on the shelf and in the windows.
- **Brass Light / Dark**: the trophy rail's brushed front edge, the only metal accent. In the windows it appears only where a trophy rail is drawn.
- **Aubergine Ground** (`window-ground`; `window-ground-light`): the setup windows' page.
- **Plum Plate** (`window-plate`): the picker's shelf plate and the settings' level diagram, the darkest stock in the shop.
- **Plum Panel / Panel Hover / Sunk** (`window-panel`, `window-panel-hover`, `window-sunk`, with light variants): tiles and settings groups, their hover, and the inset ground of fields and switches.
- **Plum Lines** (`window-line`, `window-line-strong`, `window-line-hover`): hairlines between rows, control outlines, and their hover. Line Hover also prints disabled divider letters and stepper buttons.
- **Window Text / Muted** (`window-text`, `window-muted`, with light variants): reading text and hints, counts and meta.

### Named Rules
**The Printed Colour Rule.** Rarity colour appears only as printed ink on a grade label: never as glow, border tint, or background wash. In the setup windows the same holds: a rarity filter chip carries a tiny printed tier block (9×14, 2px corners), never a tinted chip.

**The Ungraded Rule.** An achievement with no known rarity gets an unprinted tier block (card stock with muted "Ungraded" text). It never borrows a tier colour.

**The One Sticker Rule.** In the setup windows, lemon means "on the trophy shelf" and nothing else. Selection, focus and active state are lilac-grey, never lemon.

**The Shelf Stays Dark Rule.** A drawing of the shelf inside a setup window keeps the dark palette in light mode, as the shelf does.

## Typography

**Grade Font:** Barlow Condensed (self-hosted woff2 in `src/fonts`, falling back to Segoe UI)
**Body Font:** Segoe UI (system)

**Character:** Condensed bold caps for the printed grade, like a grading label. The plain Windows system face is used for everything the user reads in sentences. The setup windows extend the same pairing: Barlow Condensed caps become the divider cards' hand.

### Hierarchy
- **Grade tier** (700, 8.5px, 0.05em, uppercase): the tier name in the label's colour block.
- **Grade percent** (600, 10px, tabular numerals): the unlock percentage under it.
- **Title** (600, 14px, balanced wrap): the achievement name on the certificate.
- **Body** (400, 12px): the description.
- **Meta** (400, 11–11.5px, Ink Muted): the game name under the title, and the unlock date.

### Hierarchy (setup windows)
- **Window display** (Barlow Condensed 700, 24px, 1.1, 0.03em, uppercase): the window title ("Trophy shelf", "Settings").
- **Window headline** (Barlow Condensed 700, 18px, 0.03em, uppercase): divider-card headings. Catalog letter cards set at 19px; an opened game's title at 22px. A count inside a divider card is 14px at 0.7 opacity.
- **Divider letter** (Barlow Condensed 700, 13px): the A to Z rail.
- **Figure** (Barlow Condensed 700, 16px, tabular numerals): every count and value (placed, shelves, levels, size), at 17–20px where it leads a header.
- **Body** (Segoe UI 400, 13px, 1.4): window reading text. Labels and control text at 600.
- **Hint / Meta** (Segoe UI 400, 12px, Window Muted): setting hints, sleeve meta, tile meta.

### Named Rules
**The No Eyebrow Rule.** The certificate opens with the title. The game name goes under it as a plain meta line, never as a small tracked caps label above it. In the setup windows, a heading is a divider card, never a tracked caps label above one.

## Layout

It's a fixed-size window (520px wide, 112px per level; these must match `SHELF_WIDTH` and `ROW_HEIGHT` in `src-tauri/src/lib.rs`), anchored bottom-right. Levels stack bottom-up: trophy rails on top, recent rails below. Each level is a slab row 90px tall, a 12px rail and a 10px gap. Up to six slabs per rail are centred, 14px apart. The certificate floats above the hovered slab and stays inside the window.

### Setup windows

**Picker** (opens at 860×640, resizable). Three bands stacked down a single column capped at 1240px with a 24px gutter (14px under 700px wide), so a maximised window doesn't sprawl:
1. **Shelf plate**: title and hint, the placed count, a shelves stepper and Clear, then the trophy rails drawn at compact scale (50×71 slabs, 10px apart), stacked top first and scrolling past 202px (100px on a window shorter than 520px).
2. **Filter bar**: two rows of pills. A Games / Achievements switch, search, launcher pills, sort; then rarity chips, a Platinum chip, a shelf filter and a "Clear filters" link. Under 700px each row stays on one line and scrolls sideways with a fading right edge.
3. **The bin**, which scrolls under both. In the catalog view a 38px sticky A to Z divider rail runs down the left (30px narrow) with scroll-spy, beside cover sleeves in an auto-fill grid (min 196px, 150px narrow; 22px rows, 16px columns) under one divider card per letter. A game opened from the catalog shows a back pill, its cover at 164px (120px narrow) and title, then its achievement tiles. The achievements view groups tiles under a divider card per game. Tiles fill an auto-fill grid (min 300px, 10px gaps).

**Settings** (opens at 420×860). A single padded column (20px 18px): the title, then groups, each a divider-card tab standing on a 3px divider rule above an 8px panel of setting rows (12px 14px, hairline between rows). Shelf levels sets a level diagram (104px wide) beside its steppers. A plain group without a tab holds Reset, and the version sits quietly centred at the foot.

## Elevation & Depth

Depth comes from real contact shadows, always offset downward with a soft blur. The slab sits on the rail, the rail casts onto the wallpaper, and the certificate floats above. Nothing uses zero-offset halos.

The setup windows follow the same physics: sleeves and the sticker cast downward shadows, the shelf plate casts onto the filter bar, and the settings diagram is a sunken well. Tiles and settings groups are flat tonal panels on the ground; they change tone on hover, not height.

### Shadow Vocabulary
- **Slab rest** (`0 6px 10px -3px rgba(0,0,0,.5)`), plus inner acrylic edges (a 1px inset ring and a top highlight).
- **Slab lifted** (`0 14px 16px -6px rgba(0,0,0,.55)`): on hover, together with an 8px lift.
- **Rail** (`0 5px 9px -2px rgba(0,0,0,.55)`).
- **Certificate** (`0 10px 24px -4px rgba(0,0,0,.55)`).
- **Placard** (`0 5px 9px -3px rgba(0,0,0,.5)`).

### Shadow Vocabulary (setup windows)
- **Sleeve rest** (`0 12px 20px -10px rgba(0,0,0,.8)`; by daylight `0 10px 18px -10px rgba(45,25,55,.5)`): a cover sleeve sitting in the bin.
- **Sleeve lifted** (`0 18px 24px -12px rgba(0,0,0,.85)`): on hover or focus, with a 4px lift.
- **Shelf plate** (`0 1px 0` Plum Line, then `0 10px 18px -12px rgba(0,0,0,.7)`): the plate over the filter bar.
- **Sticker** (`0 2px 4px rgba(0,0,0,.35)`): the price sticker stuck on a sleeve or tile.
- **Divider pop** (`0 3px 6px -2px rgba(0,0,0,.5)`): the current letter's card, with a 7px nudge outward.
- **Carried slab** (`0 18px 22px -6px rgba(0,0,0,.6)`): a slab being dragged between rails.
- **Diagram well** (`inset 0 1px 4px rgba(0,0,0,.7)`): the settings' level diagram.

## Shapes

On the shelf, corners are small and get rounder with size: labels 2px, rails 3px, slabs 5px, certificate 6px. There are no pills and no circles on the shelf. The rail's slot is a dark channel along its top face that stops 14px short of each end.

The setup windows have their own form language. Small controls are pills (999px): buttons, segmented switches, chips, search, selects, steppers, the slider track and the switches. Round shapes are reserved for the price sticker, stepper buttons inside their pill, the reset-size icon button, the slider grip and the switch knob. Divider cards are tabs rounded only on their outer edge: section headings round their top corners (6px 6px 0 0) and sit on a 3px rule; A to Z letters round their right corners (0 6px 6px 0) against the bin. Tiles and settings groups are 8px; a group under a tab squares its top-left corner where the tab stands. Cover sleeves keep a near-square 3px corner, like a printed sleeve. Shelf drawings inside the windows keep the shelf's shapes.

## Components

### Slab (signature)
- **Build:** clear acrylic (white gradient over a 22% dark tint so it reads on light wallpaper), 4px padding, grade label above a 54px insert, and a soft top glare on the face.
- **Seating:** the foot sinks 4px into the rail (the rail stacks above the slab).
- **Hover:** it lifts 8px out of the slot with a deeper shadow (0.28s, expo-out `cubic-bezier(0.16,1,0.3,1)`).
- **New unlock:** it drops into the slot from above with a 3px overshoot (0.6s, backwards fill so hover still works afterwards).
- **Legendary:** one glare sweep across the acrylic on hover and on landing. Never looped.
- **Fallback icon:** a drawn line-art cup (SVG, 1.6 stroke), never an emoji.

### Grade label
- A two-tone block: the tier name printed on the tier colour, above the percent on card stock. It's stacked on slabs (22px). On the certificate it's split: the tier as a small foil chip (12px), the percent beside it in light text on the black (16px, with a muted "of players"). In the picker it runs side by side on tiles (18px, 11px type) and stacked on compact slabs (18px).

### Rail
- A matte black bar (12px) with a lit top edge and a slot channel. The trophy rail adds a 5px brushed-brass front edge.

### Certificate (tooltip)
- Black anodised stock (the rail's face with its lit top edge), 6px radius, 264px wide, light text. The game banner bleeds across the top and fades into the black at its foot.
- The achievement icon, in the slab's dark insert with an acrylic edge, rises 26px into the banner's foot; the title and a "Game · date" meta line sit beside it.
- Then the description (omitted when there is none), and a foot below a hairline: the grade chip and percent on the left, the Platinum chip on the right when the achievement completed its game. Ungraded is an outlined chip with a quiet "No rarity data".
- It fades in and rises 4px when shown (off under reduced motion).

### Placard
- Empty and error messages stand on the rail as a small card-stock placard in Ink. They're never loose text over the wallpaper. The picker's empty first rail shows the same placard.

### Buttons (setup windows)
- **Shape:** a pill (999px), 32px tall, 15px side padding, 600 weight.
- **Default:** transparent with a Plum Line Strong outline and Window Text.
- **Hover / Active:** the outline lightens to Line Hover and the fill becomes Panel Hover; pressed sinks to Sunk. Disabled drops to 40% opacity.
- **Armed:** a two-step destructive button (Reset) turns into a divider card on its first click and keeps its width.
- **Link:** a quiet text action ("Clear filters") underlined in Line Hover, the underline taking the text colour on hover.
- **Focus:** a 2px lilac-grey outline at 2px offset, across every control in the windows.

### Stepper (setup windows)
- One pill holding an optional muted label, a round minus, the count as a Figure, and a round plus. A button hovered turns into a small lilac-grey disc; at its limit it prints in Line Hover.

### Segmented switch and chips (setup windows)
- **Segmented:** a pill outline holding pill segments (26px). The chosen segment is a divider card; the rest are muted and brighten on hover. A segment may carry a Figure count at 75% opacity.
- **Chips:** 28px outlined pills in muted text; pressed, a chip fills with the divider and takes Divider Ink. A rarity chip leads with a printed tier swatch; the Platinum chip with the platinum swatch.

### Inputs / Fields (setup windows)
- **Style:** pills on the Sunk ground with a Line Strong outline; search carries a 14px line-art magnifier inside its left edge, selects a drawn chevron on the right.
- **Focus:** the outline becomes the divider with a 1px divider ring.
- **Switch:** a 40×22 pill drawn over the native checkbox; checked, it fills with the divider and the knob slides 18px and turns Divider Ink.
- **Slider:** a 6px pill track filled with the divider up to the value, with a round card-stock grip that grows 12% on hover.

### Divider card (signature, setup windows)
- A lilac-grey tab in Barlow Condensed caps that heads a section and stands on a 3px divider rule. It heads each letter in the catalog, each game in the achievements view, and each settings group.

### A to Z divider rail (signature, picker)
- A sticky column of letter tabs down the left of the catalog. Live letters are faint divider cards (26% divider) that deepen on hover and jump the bin to their letter; letters with no games print dim in Line Hover and can't be pressed.
- **Scroll-spy:** the letter at the top of the bin pops out as a full divider card, nudged 7px outward with the divider pop shadow (0.22s expo-out).

### Cover sleeve (picker)
- The game's cover at its own 460:215 ratio in a 3px sleeve with the sleeve shadow, its name (600, 14px, one line) and a muted "Steam · 12 unlocked" meta line under it. It lifts 4px on hover or focus.
- **No cover:** a drawn record in lilac-grey with the game's letter on its label, never a placeholder image.
- **Sticker:** when the game has trophies up, a 56px lemon sticker tilted −9° overhangs the top-right corner, printing the count and "on shelf".

### Achievement tile (picker)
- **Furniture:** an 8px Panel tile (10px 12px) with a 48px dark insert, then the title, one line of muted description, and a foot with the meta line and the grade labels. Every tile keeps the same furniture so the grid reads evenly.
- **Long description:** hovering or focusing opens the full text over the tiles below on a Panel Hover slip with a soft drop shadow.
- **On shelf:** the tile takes a lemon edge and an 8% lemon wash, and a 48px "On shelf" sticker slaps on at the top-right, tilted −10°, scaling in from 40% (0.32s expo-out).

### Settings group (settings)
- A divider-card tab on a divider rule over an 8px Panel body of setting rows: a 600 label with a muted 12px hint on the left, the control on the right.
- **Level diagram:** a tiny drawing of the shelf on the plum plate (104px, 6px corners, sunken), stacked bottom-up. Trophy levels carry a brass rail edge and brass label bands, recent levels card-stock bands; a new level drops in with the slot-in motion.

### Status slip (setup windows)
- Errors print on a card-stock slip in Ink (6px corners, placard shadow), as the shelf's placards do.

## Do's and Don'ts

### Do:
- **Do** keep the shelf's colour to the printed rarity tiers. Every other shelf surface is acrylic, black rail, brass, or card stock.
- **Do** put any text that sits over the wallpaper on an opaque surface: card stock, or the black anodised certificate.
- **Do** make every motion answer a user action or a change: hover lift, new-unlock drop, legendary glare.
- **Do** mark every selected or active state in the setup windows with the lilac-grey divider and Divider Ink.
- **Do** keep lemon for "on the trophy shelf" alone: the sleeve sticker, the tile sticker and the chosen tile's edge.
- **Do** draw the shelf inside a setup window exactly as on the shelf (rail, acrylic, grade, placard), in the dark palette in both modes.
- **Do** set every count and value in the setup windows as a Barlow Condensed figure with tabular numerals.

### Don't:
- **Don't** loop any animation; the widget is on screen all day, and the setup windows follow the same grammar (slot-in, sleeve lift, divider pop).
- **Don't** use glows, neon edges, or glassy blur. `backdrop-filter` can't see the desktop through the transparent window anyway.
- **Don't** use emoji or Unicode glyphs as icons.
- **Don't** put an eyebrow label above a heading.
- **Don't** bring pills or circles onto the shelf; they belong to the setup windows' controls and sticker.
- **Don't** use brass in the setup windows except where a trophy rail is drawn.
- **Don't** tint a chip, tile or sleeve with a rarity colour; rarity prints only as a tier block.
