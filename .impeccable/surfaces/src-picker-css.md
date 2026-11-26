---
version: 1
slug: "src-picker-css"
primary_target: "src/picker.css"
related_targets: ["src/picker.ts","picker.html","src/settings.css","src/settings.ts","settings.html","src/window.css"]
---

# Setup windows: trophy picker and settings

Scope: picker.html (src/picker.css, src/picker.ts) and settings.html (src/settings.css, src/settings.ts), sharing src/window.css. Mode: Operate. They open from the tray, are used now and then, and sit next to the always-on shelf.

Audience and job: the one gamer building their trophy shelf. In the picker they search for a known achievement and also browse for rare ones (sort by newest or rarest), and they watch the rail fill up. In settings they set the level counts, display, size, and on-top/autostart. Constraints: keep every existing control and behaviour, window sizes 860x640 / 420x600, the tier colours, and the Printed Colour Rule.

## Direction contract

THESIS: The setup windows are the workbench behind the shelf: the real rail is drawn in the window and fills as you pick. They refuse the generic dark settings panel with gold accents.

OWN-WORLD: Graphite anodised ground (the rail's material), hairline machined edges, card-stock printed grade labels, and brass reserved for "on the trophy shelf": chosen state, the primary count, focus. Barlow Condensed only for printed grades and numerals; Segoe UI for everything else.

STORY: The user sees their trophy rail at the top with empty slots, searches or sorts, clicks achievements to seat them, and clicks a seated slab to take it off. In settings the stack of rails redraws as levels change.

FIRST VIEWPORT: Picker: a recessed bench band across the top with the trophy rail(s) of compact slabs and empty slots, title and count left, Clear right. Below it a toolbar (launcher tabs, search, sort), then the game list and the achievement grid. Settings: a bench well holding the mini rail stack beside the level steppers, then the window controls.

FORM: Workbench and rail preview, shaped directly from the user's answers (no concept round; a narrow request inside the established Grading Rail world). seed: none (pinned) "Yes, preview the rail" / "Yes, draw the levels" / "Both equally". Signature interaction: a picked achievement drops into the next slot of the preview rail. Motion grammar: slot-in drop, hover lift, nothing loops.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
