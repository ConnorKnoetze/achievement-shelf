---
name: Achievement Shelf
description: A desktop display rail of graded achievement slabs.
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
rounded:
  label: "2px"
  rail: "3px"
  slab: "5px"
  card: "6px"
spacing:
  slab-pad: "4px"
  slab-gap: "14px"
  rail-gap: "10px"
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
---

# Design System: Achievement Shelf

## Overview

**Creative North Star: "The Grading Rail"**

Every achievement is a graded collectible. It's sealed in a clear acrylic slab with a printed grade label, and stands in a matte black display rail fixed to the bottom-right of the user's desktop. The user's own wallpaper is the ground, seen through the acrylic. The system is made of three real materials (clear acrylic, black anodised rail, off-white card stock) plus the printed rarity colour. Nothing glows and nothing is decoration. The grade label is the one place colour appears, so rarity is the first thing the eye reads.

The widget is on screen all day over unknown wallpaper, so every surface has to hold up over both light and dark backgrounds, and it stays still unless the user touches it.

**Key Characteristics:**
- Clear acrylic slabs, 62×88, each with a two-tone grade label and a dark icon insert.
- Matte black rails with a slot channel; the slab's foot sits down in the slot.
- Off-white card stock for every printed surface: grade labels and placards. The tooltip certificate is cut from the rail's black anodised stock, with its grade printed on card stock.
- Colour comes only from the five rarity tiers.
- Motion only happens when the user acts or something changes: lift on hover, a drop into the slot for new unlocks.

## Colors

The palette is neutral material (acrylic, black rail, card stock) plus five rarity colours.

### Primary
- **Rarity tiers** (common, uncommon, rare, epic, legendary): printed only as the tier block of a grade label, on slabs and on the certificate. They're shared with the picker's rarity labels and are a brand commitment.

### Neutral
- **Card Stock**: the ground of every printed surface (grade percent, placard).
- **Ink / Ink Muted**: text on card stock. Muted is for meta lines (game name, date, "Ungraded").
- **Rail Top / Face / Base**: the rail's lit top edge, its front face, and its shadowed foot.
- **Insert**: the dark well behind each icon.
- **Brass Light / Dark**: the trophy rail's brushed front edge, the only metal accent.

### Named Rules
**The Printed Colour Rule.** Rarity colour appears only as printed ink on a grade label: never as glow, border tint, or background wash.

**The Ungraded Rule.** An achievement with no known rarity gets an unprinted tier block (card stock with muted "Ungraded" text). It never borrows a tier colour.

## Typography

**Grade Font:** Barlow Condensed (self-hosted woff2 in `src/fonts`, falling back to Segoe UI)
**Body Font:** Segoe UI (system)

**Character:** Condensed bold caps for the printed grade, like a grading label. The plain Windows system face is used for everything the user reads in sentences.

### Hierarchy
- **Grade tier** (700, 8.5px, 0.05em, uppercase): the tier name in the label's colour block.
- **Grade percent** (600, 10px, tabular numerals): the unlock percentage under it.
- **Title** (600, 14px, balanced wrap): the achievement name on the certificate.
- **Body** (400, 12px): the description.
- **Meta** (400, 11–11.5px, Ink Muted): the game name under the title, and the unlock date.

### Named Rules
**The No Eyebrow Rule.** The certificate opens with the title. The game name goes under it as a plain meta line, never as a small tracked caps label above it.

## Layout

It's a fixed-size window (520px wide, 112px per level; these must match `SHELF_WIDTH` and `ROW_HEIGHT` in `src-tauri/src/lib.rs`), anchored bottom-right. Levels stack bottom-up: trophy rails on top, recent rails below. Each level is a slab row 90px tall, a 12px rail and a 10px gap. Up to six slabs per rail are centred, 14px apart. The certificate floats above the hovered slab and stays inside the window.

## Elevation & Depth

Depth comes from real contact shadows, always offset downward with a soft blur. The slab sits on the rail, the rail casts onto the wallpaper, and the certificate floats above. Nothing uses zero-offset halos.

### Shadow Vocabulary
- **Slab rest** (`0 6px 10px -3px rgba(0,0,0,.5)`), plus inner acrylic edges (a 1px inset ring and a top highlight).
- **Slab lifted** (`0 14px 16px -6px rgba(0,0,0,.55)`): on hover, together with an 8px lift.
- **Rail** (`0 5px 9px -2px rgba(0,0,0,.55)`).
- **Certificate** (`0 10px 24px -4px rgba(0,0,0,.55)`).
- **Placard** (`0 5px 9px -3px rgba(0,0,0,.5)`).

## Shapes

Corners are small and get rounder with size: labels 2px, rails 3px, slabs 5px, certificate 6px. There are no pills and no circles. The rail's slot is a dark channel along its top face that stops 14px short of each end.

## Components

### Slab (signature)
- **Build:** clear acrylic (white gradient over a 22% dark tint so it reads on light wallpaper), 4px padding, grade label above a 54px insert, and a soft top glare on the face.
- **Seating:** the foot sinks 4px into the rail (the rail stacks above the slab).
- **Hover:** it lifts 8px out of the slot with a deeper shadow (0.28s, expo-out `cubic-bezier(0.16,1,0.3,1)`).
- **New unlock:** it drops into the slot from above with a 3px overshoot (0.6s, backwards fill so hover still works afterwards).
- **Legendary:** one glare sweep across the acrylic on hover and on landing. Never looped.
- **Fallback icon:** a drawn line-art cup (SVG, 1.6 stroke), never an emoji.

### Grade label
- A two-tone block: the tier name printed on the tier colour, above the percent on card stock. It's stacked on slabs (22px) and runs full width across the foot of the certificate (26px: tier at 13px, percent at 16px with a muted "of players").

### Rail
- A matte black bar (12px) with a lit top edge and a slot channel. The trophy rail adds a 5px brushed-brass front edge.

### Certificate (tooltip)
- Black anodised stock (the rail's face with its lit top edge), 6px radius, 264px wide, light text. The game banner bleeds across the top and fades into the black at its foot.
- The achievement icon, in the slab's dark insert with an acrylic edge, rises 26px into the banner's foot; the title and a "Game · date" meta line sit beside it.
- Then the description (omitted when there is none), and the grade label run full width across the foot. Ungraded reads "Ungraded / No rarity data".
- It fades in and rises 4px when shown (off under reduced motion).

### Placard
- Empty and error messages stand on the rail as a small card-stock placard in Ink. They're never loose text over the wallpaper.

## Do's and Don'ts

### Do:
- **Do** keep colour to the printed rarity tiers. Every other surface is acrylic, black rail, brass, or card stock.
- **Do** put any text that sits over the wallpaper on an opaque surface: card stock, or the black anodised certificate.
- **Do** make every motion answer a user action or a change: hover lift, new-unlock drop, legendary glare.

### Don't:
- **Don't** loop any animation; the widget is on screen all day.
- **Don't** use glows, neon edges, or glassy blur. `backdrop-filter` can't see the desktop through the transparent window anyway.
- **Don't** use emoji or Unicode glyphs as icons.
- **Don't** put an eyebrow label above a heading.
