import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { dropPlace, findPlace, moveTo, type LevelGeometry, type Place } from "./levels";
import { gradeLabel, platinumLabel } from "./rarity";

type Source = "steam" | "gog";
type Sort = "newest" | "rarest" | "platinum";
const SORTS: readonly Sort[] = ["newest", "rarest", "platinum"];

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

/** One tab per launcher, in display order. */
const TABS: { source: Source; label: string; missing: string }[] = [
  { source: "steam", label: "Steam", missing: "Steam not found on this PC" },
  { source: "gog", label: "GOG", missing: "GOG Galaxy not found on this PC" },
];
const TAB_STORAGE_KEY = "picker-tab";
const SORT_STORAGE_KEY = "picker-sort";
const HINT_EMPTY = "Pick the achievements you are most proud of. They sit on the gold shelf above your recent unlocks.";
const HINT_PLACED = "Drag trophies on the rails to rearrange them, or click one to take it off the shelf.";

const listEl = document.getElementById("list") as HTMLElement;
const countEl = document.getElementById("count") as HTMLSpanElement;
const hintEl = document.getElementById("hint") as HTMLParagraphElement;
const railsEl = document.getElementById("rails") as HTMLElement;
const statusEl = document.getElementById("status") as HTMLParagraphElement;
const searchEl = document.getElementById("search") as HTMLInputElement;
const sortEl = document.getElementById("sort") as HTMLSelectElement;
const clearEl = document.getElementById("clear") as HTMLButtonElement;
const tabsEl = document.getElementById("tabs") as HTMLElement;
const gamesEl = document.getElementById("games") as HTMLElement;
const gameFilterEl = document.getElementById("game-filter") as HTMLInputElement;
const shelfCountEl = document.getElementById("shelf-count") as HTMLOutputElement;
const shelfAddEl = document.getElementById("shelf-add") as HTMLButtonElement;
const shelfRemoveEl = document.getElementById("shelf-remove") as HTMLButtonElement;

/** Sidebar key for the "All games" entry. */
const ALL_GAMES = "";

/** Drawn cup for achievements without an icon, as on the shelf. */
const FALLBACK_ICON =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
  '<path d="M7 4h10v5a5 5 0 0 1-10 0V4z"/><path d="M7 6H4.5a2.5 2.5 0 0 0 2.9 3.9M17 6h2.5a2.5 2.5 0 0 1-2.9 3.9"/>' +
  '<path d="M12 14v4M8.5 20h7"/></svg>';

let maxTrophies = 6;
/** Slabs per rail, as on the shelf. */
let perRail = 6;
let launchers: Source[] = [];
let items: PickerItem[] = [];
let itemsByKey = new Map<string, PickerItem>();
/** The saved trophies of each level, top first. */
let trophies: TrophyKey[][] = [];
/** Trophy levels shown on the shelf. */
let trophyRows = 1;
let chosen = new Set<string>();
/** Trophies drawn on the rail last time, so newly placed ones can drop into their slot. */
let railKeys: Set<string> | null = null;
/** The launcher tab shown, or null until the first load picks one. */
let selectedSource: Source | null = null;
/** The game picked in the sidebar (a game key), or ALL_GAMES. */
let selectedGame = ALL_GAMES;
let sort: Sort = storedSort();
/** Cards by achievement key, so toggling only updates classes instead of re-rendering. */
const cards = new Map<string, HTMLButtonElement>();
/** Sidebar buttons by game key (ALL_GAMES for the first entry). */
const gameButtons = new Map<string, HTMLButtonElement>();
const tabButtons = new Map<Source, HTMLButtonElement>();
/** Icon URLs by achievement key; null when the launcher has no icon. */
const iconCache = new Map<string, Promise<string | null>>();
let statusTimer: number | undefined;

const gameKeyOf = (item: TrophyKey) => `${item.source}:${item.gameId}`;
const keyOf = (item: TrophyKey) => `${gameKeyOf(item)}:${item.apiName}`;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className: string, text?: string) {
  const node = document.createElement(tag);
  node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function tick() {
  return el("span", "tick");
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

function storedTab(): Source | null {
  const value = readStored(TAB_STORAGE_KEY);
  return TABS.some((t) => t.source === value) ? (value as Source) : null;
}

function toSort(value: string | null): Sort {
  return SORTS.find((s) => s === value) ?? "newest";
}

function storedSort(): Sort {
  return toSort(readStored(SORT_STORAGE_KEY));
}

/** Items of the selected launcher; the sidebar, list, and search only cover these. */
function tabItems(): PickerItem[] {
  return items.filter((i) => i.source === selectedSource);
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
    iconObserver.observe(img);
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

/** Icons are loaded only when a card scrolls into view; a library can hold thousands. */
const iconObserver = new IntersectionObserver(
  (entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      const img = entry.target as HTMLImageElement;
      iconObserver.unobserve(img);
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
  if (placedCount() === 0) railRows[0].append(el("span", "rail-placard", "Pick achievements below"));
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

function updateChosen(levels: TrophyKey[][]) {
  trophies = levels;
  const keys = levels.flat();
  chosen = new Set(keys.map(keyOf));
  for (const [key, card] of cards) {
    const on = chosen.has(key);
    card.classList.toggle("chosen", on);
    card.setAttribute("aria-pressed", String(on));
  }
  const gamesWithTrophies = new Set(keys.map(gameKeyOf));
  for (const [gameKey, button] of gameButtons) {
    button.classList.toggle("has-trophy", gameKey !== ALL_GAMES && gamesWithTrophies.has(gameKey));
  }
  for (const [source, button] of tabButtons) {
    button.classList.toggle("has-trophy", keys.some((k) => k.source === source));
  }
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

function renderCard(item: PickerItem, showGame: boolean): HTMLButtonElement {
  const card = el("button", "card");
  card.type = "button";
  const onShelf = el("span", "on-shelf");
  onShelf.append(tick(), "On shelf");
  const head = el("div", "card-head");
  head.append(el("span", "card-title", item.title || item.apiName), onShelf);
  // The footer reads like the shelf's certificate: date on the left, printed grade on the right.
  const meta = el("span", "card-meta", formatDate(item.unlockedAt));
  if (showGame) meta.append(` · ${item.gameName}`);
  const foot = el("div", "card-foot");
  const grades = el("span", "card-grades");
  if (item.platinum) grades.append(platinumLabel("grade card-grade"));
  grades.append(gradeLabel(item.rarity, "grade card-grade"));
  foot.append(meta, grades);
  const text = el("div", "card-text");
  text.append(head, el("div", "card-desc", item.description || "No description"), foot);
  card.append(insert(item, true), text);
  card.addEventListener("click", () => void setTrophy(item, !chosen.has(keyOf(item))));
  return card;
}

/** Groups items by game. Items arrive newest first, so games are ordered by their latest unlock. */
function groupByGame(list: PickerItem[]): Map<string, PickerItem[]> {
  const games = new Map<string, PickerItem[]>();
  for (const item of list) {
    const key = gameKeyOf(item);
    const group = games.get(key);
    if (group) group.push(item);
    else games.set(key, [item]);
  }
  return games;
}

/** Rarest first; achievements without rarity data go last, newest first among them. */
function byRarity(a: PickerItem, b: PickerItem): number {
  if (a.rarity === null || b.rarity === null) return (a.rarity === null ? 1 : 0) - (b.rarity === null ? 1 : 0);
  return a.rarity - b.rarity;
}

/** Platinums ahead of everything else; the sort is stable, so each part keeps its newest-first order. */
function byPlatinum(a: PickerItem, b: PickerItem): number {
  return Number(b.platinum) - Number(a.platinum);
}

function selectTab(source: Source, focus = false) {
  if (source !== selectedSource) {
    selectedSource = source;
    selectedGame = ALL_GAMES;
    gameFilterEl.value = "";
    writeStored(TAB_STORAGE_KEY, source);
    renderTabs();
    renderSidebar();
    render();
    listEl.scrollTop = 0;
  }
  if (focus) tabButtons.get(source)?.focus();
}

function renderTabs() {
  tabButtons.clear();
  const buttons = TABS.map(({ source, label }, index) => {
    const selected = source === selectedSource;
    const button = el("button", selected ? "tab selected" : "tab");
    button.type = "button";
    button.id = `tab-${source}`;
    button.setAttribute("role", "tab");
    button.setAttribute("aria-selected", String(selected));
    button.setAttribute("aria-controls", "list");
    button.tabIndex = selected ? 0 : -1;
    const count = items.filter((i) => i.source === source).length;
    button.append(el("span", "tab-label", label), el("span", "tab-count", String(count)), tick());
    button.addEventListener("click", () => selectTab(source));
    // Arrow keys move between tabs, as in any tab list.
    button.addEventListener("keydown", (event) => {
      const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
      if (!step) return;
      event.preventDefault();
      selectTab(TABS[(index + step + TABS.length) % TABS.length].source, true);
    });
    tabButtons.set(source, button);
    return button;
  });
  tabsEl.replaceChildren(...buttons);
  if (selectedSource) listEl.setAttribute("aria-labelledby", `tab-${selectedSource}`);
}

function selectGame(gameKey: string) {
  selectedGame = gameKey;
  for (const [key, button] of gameButtons) {
    button.classList.toggle("selected", key === gameKey);
    button.setAttribute("aria-current", String(key === gameKey));
  }
  render();
  listEl.scrollTop = 0;
}

function gameButton(gameKey: string, name: string, count: number): HTMLButtonElement {
  const button = el("button", gameKey === selectedGame ? "game-button selected" : "game-button");
  button.type = "button";
  button.title = name;
  button.setAttribute("aria-current", String(gameKey === selectedGame));
  button.append(tick(), el("span", "game-button-name", name), el("span", "game-button-count", String(count)));
  button.addEventListener("click", () => selectGame(gameKey));
  gameButtons.set(gameKey, button);
  return button;
}

function renderSidebar() {
  gameButtons.clear();
  const list = tabItems();
  const filter = gameFilterEl.value.trim().toLowerCase();
  const buttons = [gameButton(ALL_GAMES, "All games", list.length)];
  for (const [gameKey, group] of groupByGame(list)) {
    // Keep the selected game visible even when the filter would hide it.
    if (filter && gameKey !== selectedGame && !group[0].gameName.toLowerCase().includes(filter)) continue;
    buttons.push(gameButton(gameKey, group[0].gameName, group.length));
  }
  gamesEl.replaceChildren(...buttons);
  updateChosen(trophies);
}

function emptyText(list: PickerItem[]): string {
  if (list.length) return "Nothing matches your search. Try a shorter word, or pick All games.";
  const tab = TABS.find((t) => t.source === selectedSource);
  if (tab && !launchers.includes(tab.source)) return tab.missing;
  return "No unlocked achievements found yet. New unlocks show up here as they happen.";
}

function render() {
  iconObserver.disconnect();
  cards.clear();
  const list = tabItems();
  const query = searchEl.value.trim().toLowerCase();
  const matches = list.filter(
    (i) =>
      (selectedGame === ALL_GAMES || gameKeyOf(i) === selectedGame) &&
      (!query || `${i.title} ${i.description} ${i.gameName}`.toLowerCase().includes(query)),
  );

  if (matches.length === 0) {
    listEl.replaceChildren(el("p", "empty", emptyText(list)));
    updateChosen(trophies);
    return;
  }

  const gridOf = (group: PickerItem[], showGame: boolean) => {
    const grid = el("div", "grid");
    for (const item of group) {
      const card = renderCard(item, showGame);
      cards.set(keyOf(item), card);
      grid.append(card);
    }
    return grid;
  };

  if (sort === "rarest" || sort === "platinum") {
    // One ranked list across games, so the rarest pieces, or the platinums, in the library come first.
    const section = el("section", "flat");
    section.append(gridOf([...matches].sort(sort === "rarest" ? byRarity : byPlatinum), selectedGame === ALL_GAMES));
    listEl.replaceChildren(section);
  } else {
    const sections: HTMLElement[] = [];
    for (const group of groupByGame(matches).values()) {
      const section = el("section", "game");
      const heading = el("h2", "game-name", group[0].gameName);
      heading.append(el("span", "game-count", String(group.length)));
      section.append(heading, gridOf(group, false));
      sections.push(section);
    }
    listEl.replaceChildren(...sections);
  }
  updateChosen(trophies);
}

/** First tab to show: the remembered one, else the first launcher with unlocks. */
function initialTab(): Source {
  return storedTab() ?? TABS.find((t) => items.some((i) => i.source === t.source))?.source ?? TABS[0].source;
}

async function load() {
  let data: PickerData;
  try {
    data = await invoke<PickerData>("get_unlocked");
  } catch (err) {
    listEl.replaceChildren(el("p", "empty", String(err)));
    return;
  }
  const changed = data.items.length !== items.length || data.items.some((item, i) => keyOf(item) !== keyOf(items[i]));
  showShelves(data.trophyRows, data.perRow, data.canAddTrophyRow, data.canRemoveTrophyRow);
  launchers = data.launchers;
  items = data.items;
  itemsByKey = new Map(items.map((i) => [keyOf(i), i]));
  trophies = data.trophies;
  // Choosing a trophy also fires an update; only rebuild the lists when the achievements changed.
  if (changed || cards.size === 0) {
    selectedSource ??= initialTab();
    if (!tabItems().some((i) => gameKeyOf(i) === selectedGame)) selectedGame = ALL_GAMES;
    renderTabs();
    renderSidebar();
    render();
  } else {
    updateChosen(trophies);
  }
}

sortEl.value = sort;
sortEl.addEventListener("change", () => {
  sort = toSort(sortEl.value);
  writeStored(SORT_STORAGE_KEY, sort);
  render();
  listEl.scrollTop = 0;
});
searchEl.addEventListener("input", render);
gameFilterEl.addEventListener("input", renderSidebar);
clearEl.addEventListener("click", () => {
  invoke("clear_trophies")
    .then(() => updateChosen([]))
    .catch((err) => showStatus(String(err)));
});
shelfAddEl.addEventListener("click", () => void changeShelves(true));
shelfRemoveEl.addEventListener("click", () => void changeShelves(false));
listen("achievements-updated", () => void load());
void load();
