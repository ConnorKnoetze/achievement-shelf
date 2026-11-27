import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { dropPlace, findPlace, moveTo, type LevelGeometry, type Place } from "./levels";
import { gradeLabel, platinumLabel, rarityTier } from "./rarity";

type Source = "steam" | "gog" | "ubisoft";
type Launcher = Source | "all";
type View = "games" | "achievements";
type Sort = "newest" | "rarest" | "platinum";
type GameSort = "az" | "recent" | "most";
type ShelfFilter = "any" | "on" | "off";
const SORTS: readonly Sort[] = ["newest", "rarest", "platinum"];
const GAME_SORTS: readonly GameSort[] = ["az", "recent", "most"];
const SHELF_FILTERS: readonly ShelfFilter[] = ["any", "on", "off"];

interface PickerItem {
  source: Source;
  gameId: string;
  apiName: string;
  title: string;
  description: string;
  icon: string;
  unlockedAt: number;
  rarity: number | null;
  /** The achievement that completed its game. */
  platinum: boolean;
  gameName: string;
}

interface PickerData {
  launchers: Source[];
  /** Trophies on each rail of the shelf. */
  perRow: number;
  /** Trophy levels on the shelf, and whether one can be added or removed. */
  trophyRows: number;
  canAddTrophyRow: boolean;
  canRemoveTrophyRow: boolean;
  /** Every saved trophy level, top first, including any past `trophyRows`. */
  trophies: TrophyKey[][];
  items: PickerItem[];
  /** Banner URLs (templates, for GOG) by game key, handed back to `get_banner`. */
  covers: Record<string, string>;
}

/** The part of the settings view that `change_shelf_rows` returns which the picker uses. */
interface ShelfLevels {
  trophyRows: number;
  perRow: number;
  canAddRow: boolean;
  canRemoveTrophyRow: boolean;
}

interface TrophyKey {
  source: Source;
  gameId: string;
  apiName: string;
}

/** One game in the catalog: its achievements (newest first) and those the filters keep. */
interface Game {
  key: string;
  source: Source;
  gameId: string;
  name: string;
  items: PickerItem[];
  matches: PickerItem[];
}

/** `optional` launchers are only offered when they are on this PC (or already chosen). */
const LAUNCHERS: { value: Launcher; label: string; missing?: string; optional?: boolean }[] = [
  { value: "all", label: "All" },
  { value: "steam", label: "Steam", missing: "Steam was not found on this PC." },
  { value: "gog", label: "GOG", missing: "GOG Galaxy was not found on this PC." },
  { value: "ubisoft", label: "Ubisoft", missing: "Ubisoft Connect was not found on this PC.", optional: true },
];

function sourceLabel(source: Source): string {
  return LAUNCHERS.find((l) => l.value === source)?.label ?? source;
}
/** Rarity filter chips, rarest last as the tiers climb; "ungraded" has no rarity data. */
const TIERS: { value: string; label: string }[] = [
  { value: "common", label: "Common" },
  { value: "uncommon", label: "Uncommon" },
  { value: "rare", label: "Rare" },
  { value: "epic", label: "Epic" },
  { value: "legendary", label: "Legendary" },
  { value: "ungraded", label: "Ungraded" },
];
const SORT_LABELS: Record<Sort, string> = { newest: "Newest unlocks", rarest: "Rarest first", platinum: "Platinums first" };
const GAME_SORT_LABELS: Record<GameSort, string> = { az: "A to Z", recent: "Recently unlocked", most: "Most unlocked" };

const STORE = {
  view: "picker-view",
  launcher: "picker-launcher",
  sort: "picker-sort",
  gameSort: "picker-game-sort",
  tiers: "picker-tiers",
  platinum: "picker-platinum",
  shelf: "picker-shelf-filter",
};
const HINT_EMPTY = "Open a game and click the achievements you are proudest of. They go on the brass rails above your recent unlocks.";
const HINT_PLACED = "Drag a trophy to move it along or between rails. Click one to take it off.";

const listEl = document.getElementById("list") as HTMLElement;
const countEl = document.getElementById("count") as HTMLSpanElement;
const hintEl = document.getElementById("hint") as HTMLParagraphElement;
const railsEl = document.getElementById("rails") as HTMLElement;
const statusEl = document.getElementById("status") as HTMLParagraphElement;
const searchEl = document.getElementById("search") as HTMLInputElement;
const sortEl = document.getElementById("sort") as HTMLSelectElement;
const clearEl = document.getElementById("clear") as HTMLButtonElement;
const viewsEl = document.getElementById("views") as HTMLElement;
const launchersEl = document.getElementById("launchers") as HTMLElement;
const tiersEl = document.getElementById("tiers") as HTMLElement;
const platinumEl = document.getElementById("platinum") as HTMLButtonElement;
const shelfFilterEl = document.getElementById("shelf-filter") as HTMLSelectElement;
const clearFiltersEl = document.getElementById("clear-filters") as HTMLButtonElement;
const shelfCountEl = document.getElementById("shelf-count") as HTMLOutputElement;
const shelfAddEl = document.getElementById("shelf-add") as HTMLButtonElement;
const shelfRemoveEl = document.getElementById("shelf-remove") as HTMLButtonElement;

/** Drawn cup for achievements without an icon, as on the shelf. */
const FALLBACK_ICON =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
  '<path d="M7 4h10v5a5 5 0 0 1-10 0V4z"/><path d="M7 6H4.5a2.5 2.5 0 0 0 2.9 3.9M17 6h2.5a2.5 2.5 0 0 1-2.9 3.9"/>' +
  '<path d="M12 14v4M8.5 20h7"/></svg>';
/** A drawn record for games without a cover: grooves, a label, and the spindle hole. */
const FALLBACK_COVER =
  '<svg viewBox="0 0 120 120" aria-hidden="true"><circle cx="60" cy="60" r="56" fill="#0d0910"/>' +
  '<g fill="none" stroke="#2e2336" stroke-width="1"><circle cx="60" cy="60" r="50"/><circle cx="60" cy="60" r="44"/><circle cx="60" cy="60" r="38"/><circle cx="60" cy="60" r="32"/></g>' +
  '<circle cx="60" cy="60" r="22" fill="currentColor"/><circle cx="60" cy="60" r="2.5" fill="#0d0910"/></svg>';
const BACK_ICON =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 6l-6 6 6 6"/></svg>';

let maxTrophies = 6;
/** Slabs per rail, as on the shelf. */
let perRail = 6;
let launchers: Source[] = [];
let items: PickerItem[] = [];
let itemsByKey = new Map<string, PickerItem>();
let covers: Record<string, string> = {};
/** The saved trophies of each level, top first. */
let trophies: TrophyKey[][] = [];
/** Trophy levels shown on the shelf. */
let trophyRows = 1;
let chosen = new Set<string>();
/** Trophies drawn on the rail last time, so newly placed ones can drop into their slot. */
let railKeys: Set<string> | null = null;
let statusTimer: number | undefined;

let view: View = readStored(STORE.view) === "achievements" ? "achievements" : "games";
let launcher: Launcher = toLauncher(readStored(STORE.launcher));
let sort: Sort = pick(SORTS, readStored(STORE.sort), "newest");
let gameSort: GameSort = pick(GAME_SORTS, readStored(STORE.gameSort), "az");
let tierFilter = new Set<string>(
  (readStored(STORE.tiers) ?? "").split(",").filter((t) => TIERS.some((tier) => tier.value === t)),
);
let platinumOnly = readStored(STORE.platinum) === "1";
let shelfFilter: ShelfFilter = pick(SHELF_FILTERS, readStored(STORE.shelf), "any");
/** The game opened from the catalog, by game key, or null while the catalog shows. */
let openGame: string | null = null;
/** What the catalog's search and scroll were before a game was opened, to restore on Back. */
let catalogQuery = "";
let catalogScroll = 0;

/** Achievement tiles by key, so toggling only updates classes instead of re-rendering. */
const cards = new Map<string, HTMLButtonElement>();
/** Cover stickers by game key, refreshed when the shelf changes. */
const stickers = new Map<string, HTMLElement>();
/** Icon and cover URLs; null when there is none. */
const iconCache = new Map<string, Promise<string | null>>();
const coverCache = new Map<string, Promise<string | null>>();

const gameKeyOf = (item: TrophyKey) => `${item.source}:${item.gameId}`;
const keyOf = (item: TrophyKey) => `${gameKeyOf(item)}:${item.apiName}`;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className: string, text?: string) {
  const node = document.createElement(tag);
  node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function formatDate(unix: number): string {
  if (!unix) return "Unlocked";
  return new Date(unix * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}

function showStatus(text: string) {
  statusEl.textContent = text;
  statusEl.hidden = false;
  window.clearTimeout(statusTimer);
  statusTimer = window.setTimeout(() => (statusEl.hidden = true), 4000);
}

function readStored(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStored(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Remembering the view is only a convenience.
  }
}

function pick<T extends string>(values: readonly T[], value: string | null, fallback: T): T {
  return values.find((v) => v === value) ?? fallback;
}

function toLauncher(value: string | null): Launcher {
  return pick(
    LAUNCHERS.map((l) => l.value),
    value,
    "all",
  );
}

function iconFor(item: PickerItem): Promise<string | null> {
  const key = keyOf(item);
  let url = iconCache.get(key);
  if (!url) {
    url = invoke<string | null>("get_icon", { source: item.source, gameId: item.gameId, icon: item.icon }).catch(() => null);
    iconCache.set(key, url);
  }
  return url;
}

function coverFor(game: Game): Promise<string | null> {
  let url = coverCache.get(game.key);
  if (!url) {
    url = invoke<string | null>("get_banner", {
      source: game.source,
      gameId: game.gameId,
      cover: covers[game.key] ?? null,
    }).catch(() => null);
    coverCache.set(game.key, url);
  }
  return url;
}

/** A dark insert holding the icon, or the drawn cup when there is none. */
function insert(item: PickerItem | undefined, lazy: boolean): HTMLElement {
  const box = el("span", "insert");
  if (!item) {
    box.innerHTML = FALLBACK_ICON;
    return box;
  }
  const img = el("img", "");
  img.alt = "";
  img.draggable = false;
  box.append(img);
  if (lazy) {
    img.dataset.key = keyOf(item);
    lazyObserver.observe(img);
  } else {
    void showIcon(img, item);
  }
  return box;
}

async function showIcon(img: HTMLImageElement, item: PickerItem) {
  const url = await iconFor(item);
  if (url) {
    img.src = url;
  } else {
    img.remove();
    img.parentElement?.insertAdjacentHTML("beforeend", FALLBACK_ICON);
  }
}

/** Games currently drawn in the catalog, by key, for loading their covers as they scroll in. */
const drawnGames = new Map<string, Game>();

async function showCover(img: HTMLImageElement, game: Game) {
  const url = await coverFor(game);
  const art = img.parentElement;
  if (url) {
    img.src = url;
    img.addEventListener("load", () => art?.classList.add("loaded"), { once: true });
  } else {
    img.remove();
    art?.classList.add("no-cover");
  }
}

/** Icons and covers are loaded only as they scroll into view; a library can hold thousands. */
const lazyObserver = new IntersectionObserver(
  (entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      const img = entry.target as HTMLImageElement;
      lazyObserver.unobserve(img);
      const game = img.dataset.game ? drawnGames.get(img.dataset.game) : undefined;
      if (game) {
        void showCover(img, game);
        continue;
      }
      const item = itemsByKey.get(img.dataset.key ?? "");
      if (item) void showIcon(img, item);
    }
  },
  { root: listEl, rootMargin: "300px" },
);

/*
 * The bench draws the trophy levels as the shelf does: one rail per level, stacked, each holding
 * its own trophies packed together and centred. Trophies are dragged to rearrange them, along a
 * rail or onto any other rail with room, and clicked to take them off the shelf.
 */
interface RailSlab {
  /** What the slab shows, so a redraw can tell whether it is still current. */
  look: string;
  slab: HTMLButtonElement;
}

/** Rail slabs by trophy key, reused across redraws so their icons are not loaded again. */
let railSlabs = new Map<string, RailSlab>();
const slabKey = new WeakMap<HTMLElement, TrophyKey>();
/** The slot rows of the rails drawn, top first. */
let railRows: HTMLElement[] = [];
/** Slabs of the trophies the shelf shows on each level, in order; only these can be moved. */
let shownSlabs: HTMLButtonElement[][] = [];
/** Slabs of each level's trophies no longer found on disk. The shelf skips them, but they still take room. */
let missingSlabs: HTMLButtonElement[][] = [];

const lookOf = (item: PickerItem | undefined) =>
  item ? `${item.title}|${item.apiName}|${item.rarity}|${item.platinum}|${item.icon}` : "missing";

function railSlab(key: TrophyKey, item: PickerItem | undefined, isNew: boolean): HTMLButtonElement {
  const k = keyOf(key);
  const look = lookOf(item);
  const previous = railSlabs.get(k);
  if (previous?.look === look) {
    // Only a slab made by this redraw drops in; a reused one must not replay it.
    previous.slab.classList.remove("new");
    return previous.slab;
  }
  const slab = el("button", isNew ? "pslab acrylic new" : "pslab acrylic");
  slab.type = "button";
  const title = item ? item.title || item.apiName : "An achievement no longer found";
  if (item) {
    slab.classList.add("movable");
    slab.title = `${title}\nDrag to rearrange, click to take it off the shelf`;
    slab.setAttribute("aria-keyshortcuts", "Alt+ArrowLeft Alt+ArrowRight Alt+ArrowUp Alt+ArrowDown");
  } else {
    slab.title = `${title}\nClick to take it off the shelf`;
  }
  slab.setAttribute("aria-label", `Remove ${title} from the shelf`);
  const box = insert(item, false);
  if (item?.platinum) box.append(platinumLabel("platinum-band"));
  slab.append(gradeLabel(item?.rarity ?? null, "grade pslab-grade"), box);
  slabKey.set(slab, key);
  // Pointer presses capture the pointer for dragging, so their clicks are handled in
  // `endRailDrag`; this one is the keyboard's.
  slab.addEventListener("click", (event) => {
    if (event.detail === 0) void setTrophy(key, false);
  });
  slab.addEventListener("pointerdown", (event) => startRailDrag(event, slab));
  slab.addEventListener("keydown", (event) => {
    const steps: Record<string, [number, number]> = {
      ArrowLeft: [0, -1],
      ArrowRight: [0, 1],
      ArrowUp: [-1, 0],
      ArrowDown: [1, 0],
    };
    const step = steps[event.key];
    if (!step || !event.altKey || !item) return;
    event.preventDefault();
    moveSlab(slab, step[0], step[1]);
  });
  return slab;
}

/** Puts each level's slabs (then its missing trophies) on its rail; moved slabs slide over. */
function layoutRails(levels: HTMLButtonElement[][], animate: boolean) {
  const all = levels.flat().concat(missingSlabs.flat());
  const before = animate ? new Map(all.map((slab) => [slab, slab.getBoundingClientRect()])) : null;
  railRows.forEach((row, r) => {
    const slabs = [...(levels[r] ?? []), ...(missingSlabs[r] ?? [])];
    const current = row.children;
    if (current.length !== slabs.length || slabs.some((slab, i) => current[i] !== slab)) row.replaceChildren(...slabs);
  });
  if (!before || reducedMotion()) return;
  for (const [slab, from] of before) {
    const to = slab.getBoundingClientRect();
    const dx = from.left - to.left;
    const dy = from.top - to.top;
    if (!dx && !dy) continue;
    slab.getAnimations().forEach((a) => a.cancel());
    slab.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], {
      duration: 200,
      easing: LIFT_EASING,
    });
  }
}

/** How many trophies the rails show. */
const placedCount = () => shownSlabs.flat().length + missingSlabs.flat().length;

function renderRails() {
  // A redraw would pull the carried slab out from under the pointer; it runs after the drop.
  if (railDrag) {
    railsStale = true;
    return;
  }
  const previous = railKeys;
  const next = new Map<string, RailSlab>();
  railKeys = new Set();
  shownSlabs = [];
  missingSlabs = [];
  for (let l = 0; l < trophyRows; l++) {
    // As on the shelf: the level's trophies found on disk in their saved order, up to what it
    // holds. Ones no longer found follow, so they can still be taken off.
    const level = trophies[l] ?? [];
    const placed = [
      ...level.filter((k) => itemsByKey.has(keyOf(k))),
      ...level.filter((k) => !itemsByKey.has(keyOf(k))),
    ].slice(0, perRail);
    const shown: HTMLButtonElement[] = [];
    const missing: HTMLButtonElement[] = [];
    for (const key of placed) {
      const item = itemsByKey.get(keyOf(key));
      const slab = railSlab(key, item, previous !== null && !previous.has(keyOf(key)));
      next.set(keyOf(key), { look: lookOf(item), slab });
      railKeys.add(keyOf(key));
      (item ? shown : missing).push(slab);
    }
    shownSlabs.push(shown);
    missingSlabs.push(missing);
  }
  railSlabs = next;

  railsEl.style.setProperty("--per-row", String(perRail));
  railRows = [];
  const rails: HTMLElement[] = [];
  for (let r = 0; r < trophyRows; r++) {
    const slots = el("div", "mini-slots");
    railRows.push(slots);
    const rail = el("div", "mini-rail");
    rail.setAttribute("role", "listitem");
    rail.append(slots, el("div", "rail trophy"));
    rails.push(rail);
  }
  railsEl.replaceChildren(...rails);
  layoutRails(shownSlabs, false);
  if (placedCount() === 0) railRows[0].append(el("span", "rail-placard", "Open a game below and click an achievement"));
}

/** Saves `levels` as the trophies the shelf shows on each level. */
function saveLevels(levels: HTMLButtonElement[][]) {
  const keys = levels.map((level) => level.map((slab) => slabKey.get(slab)!));
  // The same as `move_trophies`: trophies not listed stay on their level, after the listed ones.
  const listed = new Set(keys.flat().map(keyOf));
  trophies = trophies.map((level, l) =>
    l < keys.length ? [...keys[l], ...level.filter((k) => !listed.has(keyOf(k)))] : level,
  );
  for (let l = trophies.length; l < keys.length; l++) trophies.push(keys[l]);
  shownSlabs = levels;
  invoke("move_trophies", {
    levels: keys.map((level) => level.map(({ source, gameId, apiName }) => ({ source, gameId, apiName }))),
  }).catch((err) => {
    showStatus(String(err));
    void load();
  });
}

/** How many trophies each level holds besides `slab`, and how many of those can be moved. */
function levelCounts(levels: HTMLButtonElement[][], slab: HTMLButtonElement) {
  const movable = levels.map((level) => level.filter((other) => other !== slab).length);
  return { movable, others: movable.map((count, l) => count + (missingSlabs[l]?.length ?? 0)) };
}

/** Moves `slab` from the keyboard: along its rail, or to the end of the rail above or below. */
function moveSlab(slab: HTMLButtonElement, levelStep: number, indexStep: number) {
  const from = findPlace(shownSlabs, slab);
  if (railDrag || from.level < 0) return;
  const { movable, others } = levelCounts(shownSlabs, slab);
  let to: Place;
  if (levelStep) {
    const level = from.level + levelStep;
    if (level < 0 || level >= shownSlabs.length || others[level] >= perRail) return;
    to = { level, index: movable[level] };
  } else {
    const index = from.index + indexStep;
    if (index < 0 || index > movable[from.level]) return;
    to = { level: from.level, index };
  }
  const levels = moveTo(shownSlabs, slab, to);
  layoutRails(levels, true);
  saveLevels(levels);
  slab.focus();
}

/*
 * Dragging works as on the shelf. The pointer is captured on the rails, not the slab, because the
 * slab moves between rails while it is carried. The slab stays in place, faded, and a copy of it
 * follows the pointer. The rails are measured once when the drag starts (see `dropPlace`). Near
 * the top or bottom edge, the rails scroll to reach hidden levels.
 */
interface RailDrag {
  slab: HTMLButtonElement;
  pointerId: number;
  startX: number;
  startY: number;
  offsetX: number;
  offsetY: number;
  /** The pointer left a slab that cannot move, so letting go is not a click. */
  cancelled: boolean;
  ghost: HTMLElement | null;
  initial: HTMLButtonElement[][];
  levels: HTMLButtonElement[][];
  /** The rails, in their content coordinates. */
  geometry: LevelGeometry;
  /** Latest pointer position, for redrawing while the rails scroll. */
  pointerX: number;
  pointerY: number;
  scrollFrame: number;
}

const DRAG_THRESHOLD = 4;
/** How close (px) to the rails' top or bottom edge the pointer scrolls them, and the top speed (px/frame). */
const SCROLL_EDGE = 32;
const SCROLL_SPEED = 12;
const LIFT_EASING = "cubic-bezier(0.16, 1, 0.3, 1)";
const reducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;
let railDrag: RailDrag | null = null;
/** The rails changed during a drag and are redrawn after it. */
let railsStale = false;

const ghostTransform = (x: number, y: number, carried: boolean) =>
  `translate3d(${x}px, ${y}px, 0)${carried ? " rotate(-3deg) scale(1.05)" : ""}`;

function measureRails(slab: HTMLElement): LevelGeometry {
  const gap = parseFloat(getComputedStyle(railRows[0]).columnGap) || 0;
  return {
    bandEnds: railRows.map((row, r) => {
      const rail = row.parentElement!;
      const next = railRows[r + 1]?.parentElement;
      return next ? (rail.offsetTop + rail.offsetHeight + next.offsetTop) / 2 : Infinity;
    }),
    centers: railRows.map((row) => row.offsetLeft + row.offsetWidth / 2),
    pitch: slab.offsetWidth + gap,
  };
}

function startRailDrag(event: PointerEvent, slab: HTMLButtonElement) {
  if (event.button !== 0 || railDrag) return;
  const box = slab.getBoundingClientRect();
  railDrag = {
    slab,
    pointerId: event.pointerId,
    startX: event.clientX,
    startY: event.clientY,
    offsetX: event.clientX - box.left,
    offsetY: event.clientY - box.top,
    cancelled: false,
    ghost: null,
    initial: shownSlabs,
    levels: shownSlabs,
    geometry: measureRails(slab),
    pointerX: event.clientX,
    pointerY: event.clientY,
    scrollFrame: 0,
  };
  railsEl.setPointerCapture(event.pointerId);
}

/** Moves the carried slab's place to follow the pointer. */
function updateDrop(d: RailDrag) {
  const origin = railsEl.getBoundingClientRect();
  const x = d.pointerX - origin.left + railsEl.scrollLeft;
  const y = d.pointerY - origin.top + railsEl.scrollTop;
  const { movable, others } = levelCounts(d.levels, d.slab);
  const current = findPlace(d.levels, d.slab);
  const place = dropPlace(x, y, d.geometry, others, movable, perRail, current);
  if (place.level === current.level && place.index === current.index) return;
  d.levels = moveTo(d.levels, d.slab, place);
  layoutRails(d.levels, true);
}

/** Scrolls the rails while the carried slab is held near their top or bottom edge. */
function autoScroll() {
  const d = railDrag;
  if (!d?.ghost) return;
  d.scrollFrame = 0;
  const box = railsEl.getBoundingClientRect();
  const depth = Math.max(d.pointerY - (box.bottom - SCROLL_EDGE), 0) - Math.max(box.top + SCROLL_EDGE - d.pointerY, 0);
  if (!depth) return;
  const before = railsEl.scrollTop;
  railsEl.scrollTop += Math.sign(depth) * Math.min(Math.abs(depth) / SCROLL_EDGE, 1) * SCROLL_SPEED;
  if (railsEl.scrollTop === before) return;
  updateDrop(d);
  d.scrollFrame = requestAnimationFrame(autoScroll);
}

/** Ends the drag; saves the new levels when `save` and they changed, otherwise puts them back. */
function endRailDrag(save: boolean) {
  const d = railDrag;
  if (!d) return;
  railDrag = null;
  cancelAnimationFrame(d.scrollFrame);
  if (railsEl.hasPointerCapture(d.pointerId)) railsEl.releasePointerCapture(d.pointerId);
  // Let go without moving far enough to pick it up: a click.
  if (!d.ghost && !d.cancelled && save) void setTrophy(slabKey.get(d.slab)!, false);
  if (d.ghost) {
    railsEl.classList.remove("dragging");
    const changed = d.levels.some((level, l) => level.length !== d.initial[l].length || level.some((s, i) => s !== d.initial[l][i]));
    if (changed && save) saveLevels(d.levels);
    else if (changed) layoutRails(d.initial, true);
    const ghost = d.ghost;
    const finish = () => {
      ghost.remove();
      d.slab.classList.remove("drag-source");
    };
    if (reducedMotion() || !d.slab.isConnected) {
      finish();
    } else {
      // Glides into the slab's place, then shows the slab again.
      const to = d.slab.getBoundingClientRect();
      ghost
        .animate([{ transform: ghost.style.transform }, { transform: ghostTransform(to.left, to.top, false) }], {
          duration: 180,
          easing: LIFT_EASING,
          fill: "forwards",
        })
        .finished.then(finish, finish);
    }
  }
  if (railsStale) {
    railsStale = false;
    renderRails();
  }
}

railsEl.addEventListener("pointermove", (event) => {
  const d = railDrag;
  if (!d || event.pointerId !== d.pointerId) return;
  if (!d.ghost) {
    if (Math.hypot(event.clientX - d.startX, event.clientY - d.startY) < DRAG_THRESHOLD) return;
    if (!d.slab.classList.contains("movable")) {
      d.cancelled = true;
      return;
    }
    const box = d.slab.getBoundingClientRect();
    const ghost = d.slab.cloneNode(true) as HTMLElement;
    ghost.classList.remove("new");
    ghost.classList.add("drag-ghost");
    ghost.removeAttribute("title");
    ghost.style.width = `${box.width}px`;
    ghost.style.height = `${box.height}px`;
    ghost.style.transformOrigin = `${d.offsetX}px ${d.offsetY}px`;
    document.body.append(ghost);
    d.ghost = ghost;
    d.slab.classList.add("drag-source");
    railsEl.classList.add("dragging");
  }
  d.pointerX = event.clientX;
  d.pointerY = event.clientY;
  d.ghost.style.transform = ghostTransform(event.clientX - d.offsetX, event.clientY - d.offsetY, true);
  updateDrop(d);
  if (!d.scrollFrame) d.scrollFrame = requestAnimationFrame(autoScroll);
});
railsEl.addEventListener("pointerup", (event) => {
  if (railDrag && event.pointerId === railDrag.pointerId) endRailDrag(true);
});
railsEl.addEventListener("pointercancel", () => endRailDrag(false));
railsEl.addEventListener("lostpointercapture", () => endRailDrag(false));
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && railDrag) endRailDrag(false);
});


/** Shows the trophy levels and their limits, and resizes the rails to match. */
function showShelves(levels: number, rowLength: number, canAdd: boolean, canRemove: boolean) {
  trophyRows = levels;
  maxTrophies = levels * rowLength;
  perRail = rowLength;
  shelfCountEl.value = String(levels);
  shelfAddEl.disabled = !canAdd;
  shelfAddEl.title = canAdd ? "Add a trophy shelf" : "The shelf is full. Remove a recent level in Settings to make room.";
  shelfRemoveEl.disabled = !canRemove;
  shelfRemoveEl.title = canRemove ? "Remove a trophy shelf" : "The shelf keeps at least one trophy level.";
}

async function changeShelves(add: boolean) {
  try {
    const view = await invoke<ShelfLevels>("change_shelf_rows", { trophy: true, add });
    showShelves(view.trophyRows, view.perRow, view.canAddRow, view.canRemoveTrophyRow);
    updateChosen(trophies);
  } catch (err) {
    showStatus(String(err));
  }
}

function stickerText(sticker: HTMLElement, count: number) {
  sticker.hidden = count === 0;
  sticker.replaceChildren(el("b", "", String(count)), el("small", "", "on shelf"));
}

function updateChosen(levels: TrophyKey[][]) {
  trophies = levels;
  const keys = levels.flat();
  chosen = new Set(keys.map(keyOf));
  for (const [key, card] of cards) {
    const on = chosen.has(key);
    card.classList.toggle("chosen", on);
    card.setAttribute("aria-pressed", String(on));
  }
  const perGame = new Map<string, number>();
  for (const key of keys) perGame.set(gameKeyOf(key), (perGame.get(gameKeyOf(key)) ?? 0) + 1);
  for (const [gameKey, sticker] of stickers) stickerText(sticker, perGame.get(gameKey) ?? 0);
  hintEl.textContent = chosen.size ? HINT_PLACED : HINT_EMPTY;
  clearEl.disabled = chosen.size === 0;
  renderRails();
  countEl.textContent = `${placedCount()} / ${maxTrophies}`;
}

async function setTrophy(key: TrophyKey, trophy: boolean) {
  try {
    const keys = await invoke<TrophyKey[][]>("set_trophy", {
      key: { source: key.source, gameId: key.gameId, apiName: key.apiName },
      trophy,
    });
    updateChosen(keys);
  } catch (err) {
    showStatus(String(err));
  }
}

/* ---- Filters ---- */

const tierOf = (item: PickerItem) => rarityTier(item.rarity) ?? "ungraded";

/** Whether any filter that narrows achievements is on (the launcher and search aside). */
const achievementFiltersOn = () => tierFilter.size > 0 || platinumOnly || shelfFilter !== "any";

function passes(item: PickerItem): boolean {
  if (tierFilter.size && !tierFilter.has(tierOf(item))) return false;
  if (platinumOnly && !item.platinum) return false;
  if (shelfFilter === "on" && !chosen.has(keyOf(item))) return false;
  if (shelfFilter === "off" && chosen.has(keyOf(item))) return false;
  return true;
}

/** Items of the chosen launcher; everything in the list covers only these. */
function launcherItems(): PickerItem[] {
  return launcher === "all" ? items : items.filter((i) => i.source === launcher);
}

/** Groups items by game. Items arrive newest first, so games are ordered by their latest unlock. */
function gamesOf(list: PickerItem[]): Game[] {
  const games = new Map<string, Game>();
  for (const item of list) {
    const key = gameKeyOf(item);
    let game = games.get(key);
    if (!game) {
      game = { key, source: item.source, gameId: item.gameId, name: item.gameName, items: [], matches: [] };
      games.set(key, game);
    }
    game.items.push(item);
    if (passes(item)) game.matches.push(item);
  }
  return [...games.values()];
}

/** The name a record shop files a game under: without a leading "The", accents dropped. */
function filingName(name: string): string {
  return name
    .replace(/^the\s+/i, "")
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .trim();
}

/** The divider a game is filed behind: its first letter, or "#" for anything else. */
function letterOf(name: string): string {
  const first = filingName(name).charAt(0).toUpperCase();
  return first >= "A" && first <= "Z" ? first : "#";
}

const LETTERS = ["#", ..."ABCDEFGHIJKLMNOPQRSTUVWXYZ"];

/** Rarest first; achievements without rarity data go last, newest first among them. */
function byRarity(a: PickerItem, b: PickerItem): number {
  if (a.rarity === null || b.rarity === null) return (a.rarity === null ? 1 : 0) - (b.rarity === null ? 1 : 0);
  return a.rarity - b.rarity;
}

/** Platinums ahead of everything else; the sort is stable, so each part keeps its newest-first order. */
function byPlatinum(a: PickerItem, b: PickerItem): number {
  return Number(b.platinum) - Number(a.platinum);
}

function sortItems(list: PickerItem[]): PickerItem[] {
  if (sort === "rarest") return [...list].sort(byRarity);
  if (sort === "platinum") return [...list].sort(byPlatinum);
  return list;
}

const query = () => searchEl.value.trim().toLowerCase();

/** The filters in words, for an empty result: `"hades", Steam, Legendary or Epic`. */
function describeFilters(): string {
  const parts: string[] = [];
  if (query()) parts.push(`"${searchEl.value.trim()}"`);
  if (launcher !== "all") parts.push(LAUNCHERS.find((l) => l.value === launcher)!.label);
  if (tierFilter.size) parts.push(TIERS.filter((t) => tierFilter.has(t.value)).map((t) => t.label).join(" or "));
  if (platinumOnly) parts.push("Platinum");
  if (shelfFilter === "on") parts.push("on the shelf");
  if (shelfFilter === "off") parts.push("not on the shelf");
  return parts.join(", ");
}

function clearFilters() {
  tierFilter = new Set();
  platinumOnly = false;
  shelfFilter = "any";
  launcher = "all";
  searchEl.value = "";
  saveFilters();
  render(true);
}

function saveFilters() {
  writeStored(STORE.launcher, launcher);
  writeStored(STORE.tiers, [...tierFilter].join(","));
  writeStored(STORE.platinum, platinumOnly ? "1" : "0");
  writeStored(STORE.shelf, shelfFilter);
}

/* ---- Controls ---- */

const inCatalog = () => view === "games" && openGame === null;

function renderControls() {
  for (const button of viewsEl.querySelectorAll<HTMLButtonElement>("button")) {
    button.setAttribute("aria-pressed", String(button.dataset.view === view));
  }
  const games = (source: Launcher) => new Set((source === "all" ? items : items.filter((i) => i.source === source)).map(gameKeyOf)).size;
  launchersEl.replaceChildren(
    ...LAUNCHERS.filter(
      ({ value, optional }) => !optional || value === launcher || launchers.includes(value as Source),
    ).map(({ value, label }) => {
      const button = el("button", "");
      button.type = "button";
      button.setAttribute("aria-pressed", String(value === launcher));
      const count = games(value);
      button.title = `${count} ${count === 1 ? "game" : "games"}`;
      button.append(label, el("span", "seg-count", String(count)));
      button.addEventListener("click", () => {
        launcher = value;
        saveFilters();
        render(true);
      });
      return button;
    }),
  );
  for (const chip of tiersEl.querySelectorAll<HTMLButtonElement>("button")) {
    chip.setAttribute("aria-pressed", String(tierFilter.has(chip.dataset.tier!)));
  }
  platinumEl.setAttribute("aria-pressed", String(platinumOnly));
  shelfFilterEl.value = shelfFilter;
  const options = inCatalog()
    ? GAME_SORTS.map((s) => new Option(GAME_SORT_LABELS[s], s))
    : SORTS.map((s) => new Option(SORT_LABELS[s], s));
  sortEl.replaceChildren(...options);
  sortEl.value = inCatalog() ? gameSort : sort;
  const game = openGame ? drawnGames.get(openGame) : undefined;
  searchEl.placeholder = inCatalog() ? "Search games" : game ? `Search ${game.name}` : "Search achievements or games";
  searchEl.setAttribute("aria-label", searchEl.placeholder);
  clearFiltersEl.hidden = !(achievementFiltersOn() || launcher !== "all");
}

function tierChips() {
  tiersEl.replaceChildren(
    ...TIERS.map(({ value, label }) => {
      const chip = el("button", "chip");
      chip.type = "button";
      chip.dataset.tier = value;
      const swatch = el("span", value === "ungraded" ? "chip-swatch" : `chip-swatch tier-${value}`);
      swatch.setAttribute("aria-hidden", "true");
      chip.append(swatch, label);
      chip.addEventListener("click", () => {
        if (!tierFilter.delete(value)) tierFilter.add(value);
        saveFilters();
        render(true);
      });
      return chip;
    }),
  );
}

/* ---- The list ---- */

function emptyState(message: string, offerClear: boolean): HTMLElement {
  const box = el("div", "empty");
  box.append(el("p", "", message));
  if (offerClear) {
    const button = el("button", "btn", "Clear filters");
    button.type = "button";
    button.addEventListener("click", clearFilters);
    box.append(button);
  }
  return box;
}

/** The message when the launcher itself has nothing, or null when it has achievements. */
function launcherEmpty(): string | null {
  if (launcherItems().length) return null;
  const entry = LAUNCHERS.find((l) => l.value === launcher);
  if (entry?.missing && !launchers.includes(entry.value as Source)) return entry.missing;
  if (launcher === "all" && launchers.length === 0) return "No supported launcher was found on this PC.";
  return "No unlocked achievements found yet. New unlocks show up here as they happen.";
}

function renderSleeve(game: Game): HTMLButtonElement {
  const sleeve = el("button", "sleeve");
  sleeve.type = "button";
  const art = el("span", "sleeve-art");
  const fallback = el("span", "sleeve-fallback");
  fallback.innerHTML = FALLBACK_COVER;
  fallback.append(el("span", "sleeve-initial", letterOf(game.name) === "#" ? game.name.charAt(0) : letterOf(game.name)));
  const img = el("img", "");
  img.alt = "";
  img.draggable = false;
  img.dataset.game = game.key;
  art.append(fallback, img);
  lazyObserver.observe(img);
  const sticker = el("span", "sticker sleeve-sticker");
  stickers.set(game.key, sticker);
  const count = game.items.length;
  const filtered = achievementFiltersOn();
  const source = sourceLabel(game.source);
  const meta = filtered ? `${game.matches.length} of ${count} match` : `${count} unlocked`;
  sleeve.append(art, sticker, el("span", "sleeve-name", game.name), el("span", "sleeve-meta", `${source} · ${meta}`));
  sleeve.setAttribute("aria-label", `${game.name}, ${source}, ${meta}`);
  sleeve.addEventListener("click", () => openGameView(game.key));
  return sleeve;
}

/** The catalog: covers in a bin, filed behind A to Z dividers when sorted by name. */
function renderCatalog() {
  const q = query();
  const all = gamesOf(launcherItems());
  const games = all.filter((g) => (!achievementFiltersOn() || g.matches.length) && (!q || g.name.toLowerCase().includes(q)));
  const empty = launcherEmpty();
  if (empty) return listEl.replaceChildren(emptyState(empty, launcher !== "all"));
  if (!games.length) return listEl.replaceChildren(emptyState(`No games match ${describeFilters()}.`, true));

  if (gameSort === "az") games.sort((a, b) => filingName(a.name).localeCompare(filingName(b.name), undefined, { sensitivity: "base" }));
  if (gameSort === "most") games.sort((a, b) => b.items.length - a.items.length);

  const unlocked = games.reduce((n, g) => n + (achievementFiltersOn() ? g.matches.length : g.items.length), 0);
  const tally = el("p", "tally", `${games.length} ${games.length === 1 ? "game" : "games"} · ${unlocked.toLocaleString()} ${achievementFiltersOn() ? "matching" : "unlocked"}`);
  const crates = el("div", "crates");
  crates.append(tally);
  const layout = el("div", "wrap crate-layout");

  if (gameSort === "az") {
    const byLetter = new Map<string, Game[]>();
    for (const game of games) {
      const letter = letterOf(game.name);
      byLetter.set(letter, [...(byLetter.get(letter) ?? []), game]);
    }
    const nav = el("nav", "dividers");
    nav.setAttribute("aria-label", "Jump to letter");
    for (const letter of LETTERS) {
      const tab = el("button", "divider-tab", letter);
      tab.type = "button";
      tab.dataset.letter = letter;
      const filed = byLetter.get(letter);
      if (!filed) {
        tab.disabled = true;
        tab.setAttribute("aria-label", `No games under ${letter}`);
      } else {
        tab.setAttribute("aria-label", `${filed.length} ${filed.length === 1 ? "game" : "games"} under ${letter}`);
        tab.addEventListener("click", () => jumpTo(letter));
      }
      nav.append(tab);
    }
    for (const [letter, filed] of byLetter) {
      const section = el("section", "crate");
      section.id = `crate-${letter === "#" ? "num" : letter}`;
      section.dataset.letter = letter;
      const heading = el("h2", "divider-card", letter);
      const bin = el("div", "bin divider-rule");
      bin.append(...filed.map(renderSleeve));
      section.append(heading, bin);
      crates.append(section);
    }
    layout.append(nav, crates);
  } else {
    layout.classList.add("no-dividers");
    const bin = el("div", "bin");
    bin.append(...games.map(renderSleeve));
    crates.append(bin);
    layout.append(crates);
  }
  for (const game of games) drawnGames.set(game.key, game);
  listEl.replaceChildren(layout);
  spyLetter();
}

function jumpTo(letter: string) {
  const section = listEl.querySelector<HTMLElement>(`section[data-letter="${letter === "#" ? "#" : letter}"]`);
  if (!section) return;
  listEl.scrollTo({ top: section.offsetTop - 8, behavior: reducedMotion() ? "auto" : "smooth" });
}

/** Pops out the divider of the letter at the top of the bin. */
function spyLetter() {
  const tabs = listEl.querySelectorAll<HTMLButtonElement>(".divider-tab");
  if (!tabs.length) return;
  let current: string | undefined;
  for (const section of listEl.querySelectorAll<HTMLElement>("section.crate")) {
    if (section.offsetTop - listEl.scrollTop <= 40) current = section.dataset.letter;
    else break;
  }
  current ??= listEl.querySelector<HTMLElement>("section.crate")?.dataset.letter;
  for (const tab of tabs) {
    const on = tab.dataset.letter === current;
    tab.classList.toggle("current", on);
    if (on) tab.setAttribute("aria-current", "true");
    else tab.removeAttribute("aria-current");
  }
}

let spyFrame = 0;
listEl.addEventListener(
  "scroll",
  () => {
    if (spyFrame) return;
    spyFrame = requestAnimationFrame(() => {
      spyFrame = 0;
      spyLetter();
    });
  },
  { passive: true },
);

function renderTile(item: PickerItem, showGame: boolean): HTMLButtonElement {
  const tile = el("button", "tile");
  tile.type = "button";
  // Fixed furniture: title, one line of description, then the date and the printed grade.
  const head = el("span", "tile-head");
  head.append(el("span", "tile-title", item.title || item.apiName));
  const desc = el("span", "tile-desc-wrap");
  const descText = el("span", "tile-desc", item.description || "No description");
  desc.append(descText);
  const meta = el("span", "tile-meta", formatDate(item.unlockedAt));
  if (showGame) meta.append(` · ${item.gameName}`);
  const grades = el("span", "tile-grades");
  if (item.platinum) grades.append(platinumLabel("grade tile-grade"));
  grades.append(gradeLabel(item.rarity, "grade tile-grade"));
  const foot = el("span", "tile-foot");
  foot.append(meta, grades);
  const text = el("span", "tile-text");
  text.append(head, desc, foot);
  // Only a description that is cut off opens over the tiles below.
  const measure = () => tile.classList.toggle("long", descText.scrollWidth > descText.clientWidth);
  tile.addEventListener("pointerenter", measure);
  tile.addEventListener("focus", measure);
  const sticker = el("span", "sticker tile-sticker");
  sticker.append(el("small", "", "On shelf"));
  sticker.setAttribute("aria-hidden", "true");
  tile.append(insert(item, true), text, sticker);
  tile.addEventListener("click", () => void setTrophy(item, !chosen.has(keyOf(item))));
  cards.set(keyOf(item), tile);
  return tile;
}

function tileGrid(list: PickerItem[], showGame: boolean): HTMLElement {
  const grid = el("div", "tiles");
  grid.append(...list.map((item) => renderTile(item, showGame)));
  return grid;
}

function matchesQuery(item: PickerItem, withGame: boolean): boolean {
  const q = query();
  return !q || `${item.title} ${item.description}${withGame ? ` ${item.gameName}` : ""}`.toLowerCase().includes(q);
}

/** One game's achievements, opened from its cover. */
function renderGame(game: Game) {
  const layout = el("div", "wrap game-view");
  const head = el("div", "game-head");
  const back = el("button", "btn back");
  back.type = "button";
  back.innerHTML = BACK_ICON;
  back.append("All games");
  back.addEventListener("click", closeGameView);
  const cover = el("span", "game-cover sleeve-art");
  const fallback = el("span", "sleeve-fallback");
  fallback.innerHTML = FALLBACK_COVER;
  const img = el("img", "");
  img.alt = "";
  cover.append(fallback, img);
  void showCover(img, game);
  const onShelf = game.items.filter((i) => chosen.has(keyOf(i))).length;
  const title = el("div", "game-title");
  const source = sourceLabel(game.source);
  title.append(
    el("h2", "", game.name),
    el("p", "muted", `${source} · ${game.items.length} unlocked${onShelf ? ` · ${onShelf} on the shelf` : ""}`),
  );
  head.append(back, cover, title);
  layout.append(head);
  const list = sortItems(game.matches.filter((i) => matchesQuery(i, false)));
  layout.append(list.length ? tileGrid(list, false) : emptyState(`No achievements in ${game.name} match ${describeFilters()}.`, true));
  listEl.replaceChildren(layout);
}

/** Every achievement across games: by game when newest first, one ranked list otherwise. */
function renderAchievements() {
  const empty = launcherEmpty();
  if (empty) return listEl.replaceChildren(emptyState(empty, launcher !== "all"));
  const matches = launcherItems().filter((i) => passes(i) && matchesQuery(i, true));
  if (!matches.length) return listEl.replaceChildren(emptyState(`No achievements match ${describeFilters()}.`, true));
  const layout = el("div", "wrap achievements-view");
  if (sort === "newest") {
    for (const game of gamesOf(matches)) {
      const section = el("section", "game-group");
      const heading = el("h2", "divider-card", game.name);
      heading.append(el("span", "divider-count", String(game.items.length)));
      const grid = tileGrid(game.items, false);
      grid.classList.add("divider-rule");
      section.append(heading, grid);
      layout.append(section);
    }
  } else {
    layout.append(tileGrid(sortItems(matches), true));
  }
  listEl.replaceChildren(layout);
}

/** Redraws the list; `toTop` scrolls it back to the start, as after a filter change. */
function render(toTop = false) {
  lazyObserver.disconnect();
  cards.clear();
  stickers.clear();
  if (view === "games" && openGame) {
    const game = gamesOf(launcherItems()).find((g) => g.key === openGame);
    if (game) {
      drawnGames.set(game.key, game);
      renderControls();
      renderGame(game);
    } else {
      openGame = null;
    }
  }
  if (view === "achievements") {
    renderControls();
    renderAchievements();
  } else if (!openGame) {
    drawnGames.clear();
    renderControls();
    renderCatalog();
  }
  listEl.setAttribute("aria-busy", "false");
  if (toTop) listEl.scrollTop = 0;
  updateChosen(trophies);
}

function openGameView(key: string) {
  catalogQuery = searchEl.value;
  catalogScroll = listEl.scrollTop;
  searchEl.value = "";
  openGame = key;
  render(true);
}

function closeGameView() {
  openGame = null;
  searchEl.value = catalogQuery;
  render();
  listEl.scrollTop = catalogScroll;
  spyLetter();
}

function setView(next: View) {
  if (next === view && !openGame) return;
  if (openGame) {
    openGame = null;
    searchEl.value = catalogQuery;
  }
  view = next;
  writeStored(STORE.view, view);
  render(true);
}

async function load() {
  let data: PickerData;
  try {
    data = await invoke<PickerData>("get_unlocked");
  } catch (err) {
    listEl.replaceChildren(emptyState(String(err), false));
    listEl.setAttribute("aria-busy", "false");
    return;
  }
  const changed = data.items.length !== items.length || data.items.some((item, i) => keyOf(item) !== keyOf(items[i]));
  showShelves(data.trophyRows, data.perRow, data.canAddTrophyRow, data.canRemoveTrophyRow);
  launchers = data.launchers;
  items = data.items;
  itemsByKey = new Map(items.map((i) => [keyOf(i), i]));
  covers = data.covers ?? {};
  trophies = data.trophies;
  chosen = new Set(trophies.flat().map(keyOf));
  // Choosing a trophy also fires an update; only rebuild the list when the achievements changed.
  if (changed || listEl.getAttribute("aria-busy") === "true") {
    const scroll = listEl.scrollTop;
    render();
    listEl.scrollTop = scroll;
    spyLetter();
  } else {
    updateChosen(trophies);
  }
}

tierChips();
for (const button of viewsEl.querySelectorAll<HTMLButtonElement>("button")) {
  button.addEventListener("click", () => setView(button.dataset.view as View));
}
sortEl.addEventListener("change", () => {
  if (inCatalog()) {
    gameSort = pick(GAME_SORTS, sortEl.value, "az");
    writeStored(STORE.gameSort, gameSort);
  } else {
    sort = pick(SORTS, sortEl.value, "newest");
    writeStored(STORE.sort, sort);
  }
  render(true);
});
searchEl.addEventListener("input", () => render(true));
platinumEl.addEventListener("click", () => {
  platinumOnly = !platinumOnly;
  saveFilters();
  render(true);
});
shelfFilterEl.addEventListener("change", () => {
  shelfFilter = pick(SHELF_FILTERS, shelfFilterEl.value, "any");
  saveFilters();
  render(true);
});
clearFiltersEl.addEventListener("click", clearFilters);
// On a narrow window the filter rows scroll sideways: the mouse wheel scrolls them, and the
// fade at their edge goes once nothing more is hidden.
for (const row of document.querySelectorAll<HTMLElement>(".filter-row")) {
  const markEnd = () => row.classList.toggle("at-end", row.scrollLeft + row.clientWidth >= row.scrollWidth - 1);
  row.addEventListener(
    "wheel",
    (event) => {
      if (row.scrollWidth <= row.clientWidth || !event.deltaY || event.shiftKey) return;
      event.preventDefault();
      row.scrollLeft += event.deltaY;
    },
    { passive: false },
  );
  row.addEventListener("scroll", markEnd, { passive: true });
  new ResizeObserver(markEnd).observe(row);
}
clearEl.addEventListener("click", () => {
  invoke("clear_trophies")
    .then(() => updateChosen([]))
    .catch((err) => showStatus(String(err)));
});
shelfAddEl.addEventListener("click", () => void changeShelves(true));
shelfRemoveEl.addEventListener("click", () => void changeShelves(false));
// Escape steps back out of a game, unless it is cancelling a drag or clearing the search.
document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape" || railDrag || !openGame) return;
  if (event.target === searchEl && searchEl.value) return;
  closeGameView();
});
listen("achievements-updated", () => void load());
void load();
