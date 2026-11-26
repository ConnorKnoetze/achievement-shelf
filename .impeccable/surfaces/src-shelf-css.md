---
version: 1
slug: "src-shelf-css"
primary_target: "src/shelf.css"
related_targets: ["src/shelf.ts","index.html"]
---

# Shelf widget

Scope: the always-on desktop shelf (index.html, src/shelf.css, src/shelf.ts) and its hover tooltip. Mode: Experience; the achievements themselves lead.

Audience and job: one PC gamer glancing at their own desktop; they want to feel pride in a collection, with rare achievements visibly earned. Constraints: it must keep the shelf idea and the rarity tier colours, sit over any wallpaper, keep the 520px width and 112px rows (or change Rust too), and keep idle motion cheap.

## Direction contract

THESIS: Every achievement is a graded collectible, sealed in a clear acrylic slab with a printed grade label and standing in a black slotted display rail. It refuses the glowing-tile launcher overlay and the wooden bookshelf.

OWN-WORLD: The ground is the user's wallpaper, seen through clear acrylic. Rails are matte black anodised bars with a slot groove; the trophy rail is the same black with a brushed-brass front edge. Each label is off-white card stock with the rarity tier colour as its printed stripe, set in a condensed bold grading face. Colour comes only from the rarity stripes and the icons.

STORY: At a glance the user sees their collection standing in rails, and each grade label says how rare that piece is. Hovering slides the slab up out of its slot and shows its grading certificate (banner, game, title, description, date, grade).

FIRST VIEWPORT: Bottom-right of the desktop: trophy rail(s) on top, recent rails below, up to six slabs per rail, centred. Each slab is 62×88: the label band on top shows the tier and the percent, and the icon sits in a dark insert. Rails are 12px black bars that the slab feet sit into. An empty trophy rail shows the setup prompt standing on the rail.

FORM: Graded Slab Rail. It was #1 on my grounded list (IMPECCABLE'S PICK), chosen by the user over the rolled #6, Specimen Cabinet. seed:4d1a59482f840d22. Signature interaction: the slab lifts out of its slot on hover. Motion grammar: slab insertion (drop into the slot) for new unlocks, and a single acrylic glare sweep on legendary slabs only, off under reduced motion.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
