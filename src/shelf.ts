import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { dropPlace, findPlace, moveTo, type LevelGeometry } from "./levels";
import { platinumLabel, rarityTier } from "./rarity";
import { pngBase64, renderCanvas, rgbaBase64 } from "./share";

interface ShelfItem {
  source: "steam" | "gog" | "ubisoft" | "ea" | "retro" | "xbox" | "epic";
  gameId: string;
  apiName: string;
  title: string;
  description: string;
  hidden: boolean;
  icon: string;
  unlockedAt: number;
  rarity: number | null;
  /** The achievement that completed its game. */
  platinum: boolean;
  gameName: string;
  iconUrl: string | null;
}

interface Shelf {
  /** Rows of recent achievements; may be 0. */
  rows: number;
  /** Rows of the trophy shelf; at least 1. */
  trophyRows: number;
  perRow: number;
  /** The trophies on each trophy level, top first. A level may be empty. */
  trophies: ShelfItem[][];
  /** Whether each trophy level has no room for another; trophies missing from disk take room unseen. */
  trophyFull: boolean[];
  items: ShelfItem[];
  /** Banner `data:` URLs keyed by game (see `gameKeyOf`); games without one are left out. */
  banners: Record<string, string>;
}

const shelvesEl = document.getElementById("shelves") as HTMLDivElement;
const tooltipEl = document.getElementById("tooltip") as HTMLDivElement;

let knownKeys: Set<string> | null = null;
let banners: Record<string, string> = {};

/** Matches `game_key` in src-tauri/src/achievement.rs. */
const gameKeyOf = (item: ShelfItem) => `${item.source}:${item.gameId}`;
const keyOf = (item: ShelfItem) => `${gameKeyOf(item)}:${item.apiName}`;

function formatDate(unix: number): string {
  if (!unix) return "Unlocked";
  return new Date(unix * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, className: string, text?: string) {
  const node = document.createElement(tag);
  node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

/** Drawn cup for achievements without an icon. */
const FALLBACK_ICON =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
  '<path d="M7 4h10v5a5 5 0 0 1-10 0V4z"/><path d="M7 6H4.5a2.5 2.5 0 0 0 2.9 3.9M17 6h2.5a2.5 2.5 0 0 1-2.9 3.9"/>' +
  '<path d="M12 14v4M8.5 20h7"/></svg>';

/**
 * The printed grade: a tier block in the tier colour over the unlock percentage.
 * Achievements without a known rarity are "Ungraded".
 */
function gradeLabel(item: ShelfItem, className: string): HTMLElement {
  const tier = rarityTier(item.rarity);
  const label = el("div", className);
  if (tier) label.classList.add(`tier-${tier}`);
  label.append(
    el("span", "grade-tier", tier ?? "Ungraded"),
    el("span", "grade-pct", item.rarity !== null && tier ? `${item.rarity.toFixed(1)}%` : "—"),
  );
  return label;
}

const reducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const LIFT_EASING = "cubic-bezier(0.16, 1, 0.3, 1)";

/** The item each rendered slot shows. */
const itemOf = new WeakMap<HTMLElement, ShelfItem>();
/** The slot the tooltip is showing, kept across refreshes while the slot survives. */
let tooltipAnchor: HTMLElement | null = null;

/** The achievement icon in a dark insert, or the drawn cup when it has none. */
function iconInsert(item: ShelfItem, className: string, alt: string): HTMLElement {
  const insert = el("div", className);
  if (item.iconUrl) {
    const img = document.createElement("img");
    img.src = item.iconUrl;
    img.alt = alt;
    img.draggable = false;
    insert.append(img);
  } else {
    const fallback = el("div", "slab-fallback");
    fallback.innerHTML = FALLBACK_ICON;
    insert.append(fallback);
  }
  return insert;
}

/** The certificate's grade band: the printed tier block beside the share of players. */
function certificateGrade(item: ShelfItem): HTMLElement {
  const label = gradeLabel(item, "grade cert-grade");
  const pct = label.querySelector(".grade-pct")!;
  if (rarityTier(item.rarity)) pct.append(el("span", "cert-grade-note", "of players"));
  else pct.textContent = "No rarity data";
  return label;
}

/** The certificate's contents, and whether it has a banner. */
function certificate(item: ShelfItem): { children: HTMLElement[]; hasBanner: boolean } {
  const children: HTMLElement[] = [];
  const banner = banners[gameKeyOf(item)];
  if (banner) {
    const img = el("img", "cert-banner");
    img.src = banner;
    img.alt = "";
    children.push(img);
  }
  const heading = el("div", "cert-heading");
  heading.append(
    el("div", "cert-title", item.title || item.apiName),
    el("div", "cert-meta", `${item.gameName} · ${formatDate(item.unlockedAt)}`),
  );
  const head = el("div", "cert-head");
  head.append(iconInsert(item, "cert-icon", ""), heading);
  children.push(head);
  if (item.description) children.push(el("p", "cert-desc", item.description));
  const foot = el("div", "cert-foot");
  foot.append(certificateGrade(item));
  if (item.platinum) foot.append(platinumLabel("grade cert-grade cert-platinum"));
  children.push(foot);
  return { children, hasBanner: Boolean(banner) };
}

function showTooltip(item: ShelfItem, anchor: HTMLElement) {
  tooltipAnchor = anchor;
  const { children, hasBanner } = certificate(item);
  tooltipEl.classList.toggle("has-banner", hasBanner);
  tooltipEl.replaceChildren(...children);
  tooltipEl.hidden = false;

  // The slot never moves on hover (only the slab inside it lifts), so this is stable.
  const box = anchor.getBoundingClientRect();
  const width = tooltipEl.offsetWidth;
  const height = tooltipEl.offsetHeight;
  // Clears the lifted slab, which grows past the top of its slot.
  const above = box.top - height - 28;
  let left: number;
  let top: number;
  if (above >= 4) {
    left = box.left + box.width / 2 - width / 2;
    top = above;
  } else {
    // No room above, so sit beside the slab instead of covering it: to the right when it
    // fits, else to the left. The gap clears the slab's 1.2x lift scale.
    const gap = box.width * 0.1 + 10;
    const right = box.right + gap;
    left = right + width <= window.innerWidth - 8 ? right : box.left - gap - width;
    // Centred on the lifted slab, which rises 16px.
    top = box.top + box.height / 2 - 16 - height / 2;
  }
  tooltipEl.style.left = `${Math.min(Math.max(8, left), window.innerWidth - width - 8)}px`;
  tooltipEl.style.top = `${Math.min(Math.max(4, top), window.innerHeight - height - 4)}px`;
}

function hideTooltip() {
  unpin();
  tooltipAnchor = null;
  tooltipEl.hidden = true;
}

/*
 * Clicking a slab pins its certificate: it stays up after the pointer leaves, until the next
 * interaction (a click anywhere, hovering another slab, Escape, or the shelf losing focus).
 */
let pinned: HTMLElement | null = null;

function unpin() {
  pinned?.classList.remove("pinned");
  pinned = null;
}

/** Pins `slot`'s certificate, or unpins it when it is already pinned. */
function togglePin(slot: HTMLElement) {
  if (pinned === slot) return unpin();
  unpin();
  pinned = slot;
  slot.classList.add("pinned");
  showTooltip(itemOf.get(slot)!, slot);
}

// Any press other than on the pinned slab ends the pin; a press on it toggles it on release.
document.addEventListener(
  "pointerdown",
  (event) => {
    // Hovering another slab already ended the pin, so a pin here is never under the pointer.
    if (pinned && (event.target as Element).closest(".slot") !== pinned) hideTooltip();
  },
  true,
);
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && pinned) hideTooltip();
});
window.addEventListener("blur", () => {
  if (pinned) hideTooltip();
});

/*
 * Each achievement is a `.slot` holding a `.slab`. The slot is the hit box and never moves;
 * hovering it lifts the slab inside. Lifting the hovered element itself would move it out from
 * under a cursor near its lower edge, which drops it back under the cursor, and so on.
 */
function renderItem(item: ShelfItem, isNew: boolean): HTMLElement {
  const slot = el("div", "slot");
  const slab = el("div", isNew ? "slab new" : "slab");
  const tier = rarityTier(item.rarity);
  if (tier) slab.classList.add(`tier-${tier}`);
  if (item.platinum) slab.classList.add("platinum");
  const insert = iconInsert(item, "slab-insert", item.title);
  if (item.platinum) insert.append(platinumLabel("platinum-band"));
  slab.append(gradeLabel(item, "grade slab-grade"), insert);
  slot.append(slab);
  itemOf.set(slot, item);
  slot.addEventListener("mouseenter", () => {
    if (drag?.ghost) return;
    if (pinned !== slot) unpin();
    showTooltip(itemOf.get(slot)!, slot);
    if (glared !== slot) sweepGlare(slab);
    glared = slot;
  });
  slot.addEventListener("mouseleave", () => {
    if (pinned !== slot) hideTooltip();
    // Pressing a trophy captures the pointer on the shelves, which leaves and re-enters its slot
    // without the pointer moving; that must not count as lifting it again.
    if (!drag) glared = null;
  });
  // Movable slots capture the pointer for dragging, so their clicks are handled in `endDrag`.
  slot.addEventListener("click", () => {
    if (!slot.classList.contains("movable")) togglePin(slot);
  });
  return slot;
}

/*
 * Legendary and platinum slabs catch the light as they are lifted: one sweep of their `::before`
 * (see shelf.css), when the pointer comes onto the slot. It is started here rather than by
 * `:hover`, which also toggles when a press captures the pointer, so a click replayed it.
 */
/** The slot the pointer is on whose glare has already swept, if any. */
let glared: HTMLElement | null = null;

const GLARE_SWEEP: Keyframe[] = [
  { transform: "translateX(-180%) skewX(-16deg)" },
  { transform: "translateX(330%) skewX(-16deg)" },
];

function sweepGlare(slab: HTMLElement) {
  if (reducedMotion() || !slab.matches(".platinum, .tier-legendary")) return;
  slab.animate(GLARE_SWEEP, { duration: 900, easing: LIFT_EASING, pseudoElement: "::before" });
}

/** Replaces `parent`'s children with `nodes`, leaving it untouched when they already match. */
function setChildren(parent: Element, nodes: HTMLElement[]) {
  const current = parent.children;
  if (current.length === nodes.length && nodes.every((node, i) => current[i] === node)) return;
  parent.replaceChildren(...nodes);
}

/*
 * Under the mouse, the lifted slab tilts so the point beneath the cursor presses away, and its
 * glare leans the other way. The box measured is the slot's, which never moves, so the tilt
 * never feeds back into where the cursor appears to be. The transform is written straight to
 * the slab; clearing it lets the slab ease back to rest through its CSS transition.
 */
const tiltQuery = window.matchMedia("(hover: hover) and (pointer: fine) and (prefers-reduced-motion: no-preference)");
const MAX_TILT = 12; // Degrees at the slab's edges.
const TILT_PERSPECTIVE = 500;
const HOVER_SCALE = 1.2; // Scale and lift match `.slot:hover > .slab` in shelf.css.
const HOVER_LIFT = 16;
let tilted: HTMLElement | null = null;
let tiltFrame = 0;
let tiltPointer = { x: 0, y: 0, slot: null as HTMLElement | null };

function resetTilt() {
  cancelAnimationFrame(tiltFrame);
  tiltFrame = 0;
  tiltPointer.slot = null;
  if (!tilted) return;
  tilted.style.transform = "";
  tilted.style.removeProperty("--glare-x");
  tilted.style.removeProperty("--glare-y");
  tilted = null;
}

function drawTilt() {
  tiltFrame = 0;
  const slot = tiltPointer.slot;
  const slab = slot?.firstElementChild as HTMLElement | null | undefined;
  if (slab !== tilted) {
    const pointer = tiltPointer;
    resetTilt();
    tiltPointer = pointer;
  }
  if (!slot || !slab || drag?.ghost) return;
  const box = slot.getBoundingClientRect();
  const x = Math.min(Math.max((tiltPointer.x - box.left) / box.width, 0), 1) - 0.5; // -0.5 left, 0.5 right.
  const y = Math.min(Math.max((tiltPointer.y - box.top) / box.height, 0), 1) - 0.5; // -0.5 top, 0.5 bottom.
  slab.style.transform =
    `perspective(${TILT_PERSPECTIVE}px) rotateX(${(-y * 2 * MAX_TILT).toFixed(2)}deg) ` +
    `rotateY(${(x * 2 * MAX_TILT).toFixed(2)}deg) translateY(-${HOVER_LIFT}px) scale(${HOVER_SCALE})`;
  slab.style.setProperty("--glare-x", `${(-x * 24).toFixed(1)}px`);
  slab.style.setProperty("--glare-y", `${(-y * 16).toFixed(1)}px`);
  tilted = slab;
}

shelvesEl.addEventListener(
  "pointermove",
  (event) => {
    if (event.pointerType !== "mouse" || drag?.ghost || !tiltQuery.matches) return;
    const slot = (event.target as Element).closest<HTMLElement>(".slot");
    tiltPointer = { x: event.clientX, y: event.clientY, slot: slot && shelvesEl.contains(slot) ? slot : null };
    if (!tiltFrame) tiltFrame = requestAnimationFrame(drawTilt);
  },
  { passive: true },
);
shelvesEl.addEventListener("pointerleave", resetTilt);

/*
 * Trophies are dragged to rearrange them, along a trophy row or onto any other trophy row with
 * room. Recent achievements can be picked up too, but only to drag them out (see `dragOut`); they
 * stay in unlock order. The pointer is captured on the shelves element, not the slot, because
 * the slot moves between rows while it is dragged. The slot stays in place as a faded placeholder and a copy of
 * its slab follows the pointer.
 *
 * The rows are measured once when the drag starts (see `dropPlace`); measuring live would read
 * slots mid-slide and flip the drop target back and forth under a still pointer.
 */
interface Drag {
  slot: HTMLElement;
  /** Whether the slot is a trophy, which can be rearranged; a recent one can only be dragged out. */
  trophy: boolean;
  pointerId: number;
  startX: number;
  startY: number;
  offsetX: number;
  offsetY: number;
  ghost: HTMLElement | null;
  /** The trophy slots on each trophy row, top first: as picked up, and as now arranged. */
  initial: HTMLElement[][];
  levels: HTMLElement[][];
  /** The trophy rows, in `.shelves` coordinates. */
  geometry: LevelGeometry;
  /** The row the trophy was picked up from; it always has room to take it back. */
  fromLevel: number;
  /** Latest pointer position, drawn on the next frame. */
  pointerX: number;
  pointerY: number;
  frame: number;
  /** The ghost's swing: angle (deg) and its rate (deg/s), driven by the pointer's speed. */
  swing: { angle: number; rate: number; speed: number; lastX: number; lastTime: number };
  /** The images for dragging the trophy out (see `dragOut`): kept, or prepared since it was picked up. */
  share: Promise<PreparedShare> | null;
}

const DRAG_THRESHOLD = 4;
const trophyOf = new WeakMap<HTMLElement, ShelfItem>();
let perRow = 6;
/** Whether each trophy row is full, from the last shelf drawn. */
let trophyFull: boolean[] = [];
let drag: Drag | null = null;
let pendingRefresh = false;

const trophyRowItems = () => [...shelvesEl.querySelectorAll<HTMLElement>(".row.trophy .row-items")];
const trophyLevels = () =>
  trophyRowItems().map((row) => [...row.children].filter((node): node is HTMLElement => node.classList.contains("slot")));

/** Top-left of `slot`'s place in client coordinates, ignoring transforms and animations. */
function placeOf(slot: HTMLElement) {
  const origin = shelvesEl.getBoundingClientRect();
  return { left: origin.left + slot.offsetLeft, top: origin.top + slot.offsetTop };
}

/** Puts the trophy slots on the trophy rows as `levels` says; others slide over. */
function layoutTrophies(levels: HTMLElement[][]) {
  const all = levels.flat();
  const before = new Map(all.map((slot) => [slot, slot.getBoundingClientRect()]));
  for (const slot of all) slot.getAnimations().forEach((a) => a.cancel());
  trophyRowItems().forEach((row, r) => setChildren(row, levels[r] ?? []));
  if (reducedMotion()) return;
  for (const [slot, from] of before) {
    const to = slot.getBoundingClientRect();
    const dx = from.left - to.left;
    const dy = from.top - to.top;
    if (dx || dy) {
      slot.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], {
        duration: 200,
        easing: LIFT_EASING,
      });
    }
  }
}

/** The trophy rows' bands, centres and slot spacing, in `.shelves` coordinates. */
function measureTrophyRows(slot: HTMLElement): LevelGeometry {
  const rows = trophyRowItems();
  const gap = parseFloat(getComputedStyle(rows[0]).columnGap) || 0;
  return {
    bandEnds: rows.map((items, r) => {
      const row = items.parentElement!;
      return r < rows.length - 1 ? row.offsetTop + row.offsetHeight : Infinity;
    }),
    centers: rows.map((items) => items.offsetLeft + items.offsetWidth / 2),
    pitch: slot.offsetWidth + gap,
  };
}

/** The trophy rows with the dragged slot where the pointer would drop it, or `d.levels` unchanged. */
function dropLevels(d: Drag, x: number, y: number): HTMLElement[][] {
  const origin = shelvesEl.getBoundingClientRect();
  const movable = d.levels.map((level) => level.filter((slot) => slot !== d.slot).length);
  // A row other than its own is full when the backend says so, which counts trophies not shown.
  const others = movable.map((count, l) => (l !== d.fromLevel && trophyFull[l] ? perRow : count));
  const current = findPlace(d.levels, d.slot);
  const place = dropPlace(x - origin.left, y - origin.top, d.geometry, others, movable, perRow, current);
  if (place.level === current.level && place.index === current.index) return d.levels;
  return moveTo(d.levels, d.slot, place);
}

/** `angle` is the carried tilt in degrees; leave it out for the slab seated in its slot. */
const ghostTransform = (x: number, y: number, angle?: number) =>
  `translate3d(${x}px, ${y}px, 0)${angle === undefined ? "" : ` rotate(${angle.toFixed(2)}deg) scale(1.05)`}`;

/*
 * The carried slab hangs from the point it was grabbed by and swings as it is moved: a damped
 * spring pulls its angle towards one set by the pointer's horizontal speed, so it trails behind
 * a quick move and sways back and forth when the pointer stops.
 */
const CARRY_TILT = -3; // Resting angle while carried.
const SWING_STIFFNESS = 90; // 1/s²: about 1.5 swings a second.
const SWING_DAMPING = 5; // 1/s: settles in a few swings.
const SWING_PER_SPEED = 0.02; // Degrees of lean per px/s of pointer speed.
const MAX_SWING = 22;
const SPEED_SMOOTHING = 0.05; // Seconds; evens out uneven pointer events.

/** Advances the swing to `now`; returns whether it is still moving. */
function stepSwing(d: Drag, now: number): boolean {
  const s = d.swing;
  const dt = Math.min((now - s.lastTime) / 1000, 1 / 30);
  if (dt <= 0) return true;
  const raw = (d.pointerX - s.lastX) / dt;
  s.speed += (raw - s.speed) * (1 - Math.exp(-dt / SPEED_SMOOTHING));
  s.lastX = d.pointerX;
  s.lastTime = now;
  const target = Math.min(Math.max(s.speed * SWING_PER_SPEED, -MAX_SWING), MAX_SWING);
  s.rate += (SWING_STIFFNESS * (target - s.angle) - SWING_DAMPING * s.rate) * dt;
  s.angle += s.rate * dt;
  const resting = Math.abs(s.angle) < 0.05 && Math.abs(s.rate) < 0.5 && Math.abs(s.speed) < 1;
  if (resting) s.angle = s.rate = s.speed = 0;
  return !resting;
}

function startDrag(event: PointerEvent, slot: HTMLElement) {
  if (event.button !== 0 || drag) return;
  const box = slot.firstElementChild!.getBoundingClientRect();
  const levels = trophyLevels();
  drag = {
    slot,
    trophy: trophyOf.has(slot),
    pointerId: event.pointerId,
    startX: event.clientX,
    startY: event.clientY,
    offsetX: event.clientX - box.left,
    offsetY: event.clientY - box.top,
    ghost: null,
    initial: levels,
    levels,
    geometry: measureTrophyRows(slot),
    fromLevel: findPlace(levels, slot).level,
    pointerX: event.clientX,
    pointerY: event.clientY,
    frame: 0,
    swing: { angle: 0, rate: 0, speed: 0, lastX: event.clientX, lastTime: performance.now() },
    share: null,
  };
  shelvesEl.setPointerCapture(event.pointerId);
}

/** Picks the dragged slab up: a copy of it follows the pointer, from (`x`, `y`). */
function liftSlab(d: Drag, x: number, y: number) {
  hideTooltip();
  resetTilt();
  glared = null; // Carried off, so it sweeps again when next lifted.
  const ghost = d.slot.firstElementChild!.cloneNode(true) as HTMLElement;
  ghost.classList.remove("new");
  ghost.classList.add("drag-ghost");
  ghost.style.transformOrigin = `${d.offsetX}px ${d.offsetY}px`; // Swings from where it is held.
  ghost.style.transform = ghostTransform(x - d.offsetX, y - d.offsetY, CARRY_TILT + d.swing.angle);
  document.body.append(ghost);
  d.swing.lastX = x;
  d.swing.lastTime = performance.now();
  d.ghost = ghost;
  d.share = prepareShare(d.slot, itemOf.get(d.slot)!);
  d.share.catch((err) => console.error("cannot prepare the achievement image:", err));
  d.slot.classList.add("drag-source");
  shelvesEl.classList.add("dragging");
  reportHitRect();
}

/** Glides the ghost into the slot's place, then shows the slot again. */
function settleGhost(ghost: HTMLElement, slot: HTMLElement) {
  const finish = () => {
    ghost.remove();
    // The slab may have been dragged out and brought back, and be carried again by now.
    if (drag?.slot !== slot) slot.classList.remove("drag-source");
  };
  if (reducedMotion() || !slot.isConnected) return finish();
  const { left, top } = placeOf(slot);
  ghost
    .animate([{ transform: ghost.style.transform }, { transform: ghostTransform(left, top) }], {
      duration: 180,
      easing: LIFT_EASING,
      fill: "forwards",
    })
    .finished.then(finish, finish);
}

/*
 * Dragging a trophy off the shelf turns it into an image file the OS carries on, so it can be
 * dropped into a chat app such as Discord, or anywhere else that takes image files. The image
 * is the slab standing on its rail beside its certificate. It is prepared when the trophy is
 * first picked up (see `prepareShare`), so it is usually ready by the time the pointer leaves the
 * shelf, and kept for later drags.
 */
/** How far past the shelf's edge (px) the pointer goes before the trophy is dragged out. */
const DRAG_OUT_MARGIN = 16;

interface ShareImages {
  /** The shared image: a PNG, base64. */
  image: string;
  /** What follows the cursor: RGBA pixels, base64, `previewWidth` wide. */
  preview: string;
  previewWidth: number;
  /** Device pixels per CSS pixel in the preview. */
  previewScale: number;
}

/** Room around the slab in the preview, in CSS px; matches `.share-preview` in shelf.css. */
const PREVIEW_PAD = { x: 8, y: 4 };

function shareCard(slab: HTMLElement, item: ShelfItem): HTMLElement {
  const stand = el("div", "share-stand");
  const seat = el("div", "share-slab");
  seat.append(slab);
  stand.append(seat, el("div", "rail share-rail"), el("div", "share-mark", "Achievement Shelf"));
  const { children, hasBanner } = certificate(item);
  const cert = el("div", hasBanner ? "tooltip share-cert has-banner" : "tooltip share-cert");
  cert.append(...children);
  const card = el("div", "share-card");
  card.append(stand, cert);
  return card;
}

/*
 * Each achievement's images are drawn and handed to the backend once, then kept, so picking a
 * slab up again and dragging it out does neither again. They are drawn again when anything
 * drawn in them changes: the achievement, the colour scheme or the device pixel ratio (which
 * includes the shelf's zoom).
 */
interface PreparedShare {
  /** What the backend keeps the images under (`prepare_share`). */
  id: string;
  /** Device pixels per CSS pixel in the preview. */
  previewScale: number;
}

/** No more than this many achievements' images are kept; the oldest go first. */
const MAX_SHARES = 64;
/** By `shareKey`, least recently used first. */
const shares = new Map<string, Promise<PreparedShare>>();
let nextShareId = 1;

/** Lets go of the images under `keys`, here and in the backend, which deletes their files. */
function forgetShares(keys: string[]) {
  for (const key of keys) {
    const prepared = shares.get(key);
    if (!prepared) continue;
    shares.delete(key);
    prepared.then(
      ({ id }) => invoke("forget_shares", { ids: [id] }).catch(console.error),
      () => {},
    );
  }
}

/**
 * Lets go of the images of achievements no longer on the shelf, and of ones drawn differently
 * now. Run after each shelf is drawn.
 */
function pruneShares() {
  const current = new Set(
    [...shelvesEl.querySelectorAll<HTMLElement>(".slot")].map((slot) => shareKey(itemOf.get(slot)!)),
  );
  forgetShares([...shares.keys()].filter((key) => !current.has(key)));
}
const darkScheme = window.matchMedia("(prefers-color-scheme: dark)");

function shareKey(item: ShelfItem): string {
  const { source, gameId, apiName, title, description, rarity, platinum, unlockedAt, gameName, iconUrl } = item;
  const drawn = [source, gameId, apiName, title, description, rarity, platinum, unlockedAt, gameName, iconUrl];
  return JSON.stringify([...drawn, window.devicePixelRatio, darkScheme.matches]);
}

/** The achievement's images, drawn and prepared now, or kept from before. */
function prepareShare(slot: HTMLElement, item: ShelfItem): Promise<PreparedShare> {
  const key = shareKey(item);
  const kept = shares.get(key);
  if (kept) {
    // Most recently used last, so the oldest is first to go.
    shares.delete(key);
    shares.set(key, kept);
    return kept;
  }
  const id = String(nextShareId++);
  const prepared = renderShare(slot, item).then(async ({ image, preview, previewWidth, previewScale }) => {
    await invoke("prepare_share", { id, title: item.title || item.apiName, image, preview, previewWidth });
    return { id, previewScale };
  });
  prepared.catch(() => {
    if (shares.get(key) === prepared) shares.delete(key);
  });
  shares.set(key, prepared);
  if (shares.size > MAX_SHARES) forgetShares([...shares.keys()].slice(0, shares.size - MAX_SHARES));
  return prepared;
}

async function renderShare(slot: HTMLElement, item: ShelfItem): Promise<ShareImages> {
  const slab = slot.firstElementChild!.cloneNode(true) as HTMLElement;
  slab.classList.remove("new");
  slab.removeAttribute("style");
  const preview = el("div", "share-preview");
  preview.append(slab.cloneNode(true));
  // The preview follows the cursor at the size of the carried slab on screen.
  const previewScale = window.devicePixelRatio * 1.05;
  const [card, previewCanvas] = await Promise.all([
    renderCanvas(shareCard(slab, item), Math.max(2, window.devicePixelRatio)),
    renderCanvas(preview, previewScale),
  ]);
  return {
    image: pngBase64(card),
    preview: rgbaBase64(previewCanvas),
    previewWidth: previewCanvas.width,
    previewScale,
  };
}

/** The slot's key in `rendered`. */
const renderedKey = (item: ShelfItem, trophy: boolean) => `${trophy ? "t" : "r"}:${keyOf(item)}`;

/**
 * The achievement last dragged out (its key in `rendered`), and where it was held, in case it
 * is brought back.
 */
let draggedOut: { key: string; offsetX: number; offsetY: number } | null = null;

/** Hands the carried achievement to the OS as an image file, leaving its slot faded meanwhile. */
async function dragOut(d: Drag) {
  const prepared = d.share!;
  draggedOut = { key: renderedKey(itemOf.get(d.slot)!, d.trophy), offsetX: d.offsetX, offsetY: d.offsetY };
  endDrag(false, true);
  try {
    const { id, previewScale } = await prepared;
    // Held by the point the slab was picked up by, as the carried slab was.
    const hotspot = [
      Math.round((d.offsetX + PREVIEW_PAD.x) * previewScale),
      Math.round((d.offsetY + PREVIEW_PAD.y) * previewScale),
    ];
    // The slab carries on swinging off the shelf, from where it had got to.
    const { angle, rate, speed } = d.swing;
    // Brought back over the shelf, it is carried here again (`share-drag-returned`).
    const { left, top, width, height } = shelvesEl.getBoundingClientRect();
    await invoke("share_drag", {
      shelf: { x: left, y: top, width, height },
      id,
      hotspot,
      swing: reducedMotion() ? null : { angle, rate, speed },
      pixelRatio: window.devicePixelRatio,
    });
  } catch (err) {
    console.error("cannot drag out the achievement:", err);
    endDragOut();
  }
}

/** The drag out ended away from the shelf: dropped, cancelled or failed. Its slot shows again. */
function endDragOut() {
  draggedOut = null;
  for (const slot of shelvesEl.querySelectorAll<HTMLElement>(".slot.drag-source")) {
    if (slot !== drag?.slot) slot.classList.remove("drag-source");
  }
}
listen("share-drag-ended", endDragOut);

function outsideShelf(x: number, y: number) {
  const r = shelvesEl.getBoundingClientRect();
  const m = DRAG_OUT_MARGIN;
  return x < r.left - m || x > r.right + m || y < r.top - m || y > r.bottom + m;
}

/**
 * Ends the drag; saves the new order when `save` and it changed, otherwise puts it back. When
 * the trophy is dragged `out`, the OS carries the slab on, so its copy here goes and its slot
 * stays faded until that drag ends (`share-drag-ended`).
 */
function endDrag(save: boolean, out = false) {
  const d = drag;
  if (!d) return;
  drag = null;
  cancelAnimationFrame(d.frame);
  if (shelvesEl.hasPointerCapture(d.pointerId)) shelvesEl.releasePointerCapture(d.pointerId);
  let saving = false;
  // Released without moving far enough to pick it up: a click.
  if (!d.ghost && save) togglePin(d.slot);
  if (d.ghost) {
    shelvesEl.classList.remove("dragging");
    reportHitRect();
    const changed = d.levels.some(
      (level, l) => level.length !== d.initial[l].length || level.some((slot, i) => slot !== d.initial[l][i]),
    );
    if (changed && !save) layoutTrophies(d.initial);
    if (out) d.ghost.remove();
    else settleGhost(d.ghost, d.slot);
    if (changed && save) {
      saving = true;
      const levels = d.levels.map((level) =>
        level.map((slot) => {
          const { source, gameId, apiName } = trophyOf.get(slot)!;
          return { source, gameId, apiName };
        }),
      );
      invoke("move_trophies", { levels }).catch((err) => {
        console.error(err);
        void refresh();
      });
    }
  }
  // A save emits its own update once the new order is stored; refreshing before that could
  // briefly show the old order.
  if (pendingRefresh) {
    pendingRefresh = false;
    if (!saving) void refresh();
  }
}

function drawDrag(now: number) {
  const d = drag;
  if (!d?.ghost) return;
  d.frame = 0;
  // Keeps drawing while the slab swings, after the pointer has stopped too.
  if (!reducedMotion() && stepSwing(d, now)) d.frame = requestAnimationFrame(drawDrag);
  d.ghost.style.transform = ghostTransform(d.pointerX - d.offsetX, d.pointerY - d.offsetY, CARRY_TILT + d.swing.angle);
  // A recent achievement is only carried; it goes back to its own slot when dropped.
  const levels = d.trophy ? dropLevels(d, d.pointerX, d.pointerY) : d.levels;
  if (levels !== d.levels) {
    d.levels = levels;
    layoutTrophies(levels);
  }
}

/*
 * An achievement dragged out and brought back over the shelf with the button still down is
 * carried here again, from where it was and swinging as it was; a trophy can then be placed. The OS drag
 * gave no pointer events, so the drag takes on the mouse's pointer at its first event.
 */
const MOUSE_UNKNOWN = -1;

listen<{ x: number; y: number; swing: { angle: number; rate: number; speed: number } }>(
  "share-drag-returned",
  ({ payload: { x, y, swing } }) => {
    const out = draggedOut;
    draggedOut = null;
    if (!out || drag) return;
    const slot = rendered.get(out.key)?.slot;
    if (!slot?.isConnected) return;
    const levels = trophyLevels();
    const d: Drag = {
      slot,
      trophy: trophyOf.has(slot),
      pointerId: MOUSE_UNKNOWN,
      startX: x,
      startY: y,
      offsetX: out.offsetX,
      offsetY: out.offsetY,
      ghost: null,
      initial: levels,
      levels,
      geometry: measureTrophyRows(slot),
      fromLevel: findPlace(levels, slot).level,
      pointerX: x,
      pointerY: y,
      frame: 0,
      swing: { ...swing, lastX: x, lastTime: performance.now() },
      share: null,
    };
    drag = d;
    liftSlab(d, x, y);
    d.frame = requestAnimationFrame(drawDrag);
  },
);

/** Whether `event` is the dragging pointer's, taking on the mouse for a drag brought back. */
function ownsPointer(d: Drag, event: PointerEvent) {
  if (d.pointerId === MOUSE_UNKNOWN && event.pointerType === "mouse") {
    d.pointerId = event.pointerId;
    try {
      shelvesEl.setPointerCapture(event.pointerId);
    } catch {
      // Not pressed as far as the page knows; the drag carries on while the pointer is over it.
    }
  }
  return event.pointerId === d.pointerId;
}

shelvesEl.addEventListener("pointermove", (event) => {
  const d = drag;
  if (!d || !ownsPointer(d, event)) return;
  // The button came up while the window was ignoring the mouse (outside the shelf).
  if (event.buttons === 0) return endDrag(true);
  if (!d.ghost) {
    if (Math.hypot(event.clientX - d.startX, event.clientY - d.startY) < DRAG_THRESHOLD) return;
    liftSlab(d, event.clientX, event.clientY);
  }
  if (outsideShelf(event.clientX, event.clientY)) return void dragOut(d);
  d.pointerX = event.clientX;
  d.pointerY = event.clientY;
  if (!d.frame) d.frame = requestAnimationFrame(drawDrag);
});
shelvesEl.addEventListener("pointerup", (event) => {
  if (drag && ownsPointer(drag, event)) endDrag(true);
});
shelvesEl.addEventListener("pointercancel", () => endDrag(false));
shelvesEl.addEventListener("lostpointercapture", () => endDrag(false));

/** A slot that can be picked up: a trophy, or a recent achievement (`trophy` false). */
function renderMovable(item: ShelfItem, isNew: boolean, trophy: boolean): HTMLElement {
  const slot = renderItem(item, isNew);
  slot.classList.add("movable");
  if (trophy) trophyOf.set(slot, item);
  slot.addEventListener("pointerdown", (event) => startDrag(event, slot));
  return slot;
}

/*
 * Refreshes reuse the slots of unchanged achievements, so their icons are not decoded again,
 * the hovered slab stays lifted and the tooltip stays up. Updates come often: every trophy
 * reorder, row change and achievement file write by a launcher.
 */
interface Rendered {
  item: ShelfItem;
  slot: HTMLElement;
}

let rendered = new Map<string, Rendered>();

const sameItem = (a: ShelfItem, b: ShelfItem) =>
  a.title === b.title &&
  a.apiName === b.apiName &&
  a.description === b.description &&
  a.rarity === b.rarity &&
  a.platinum === b.platinum &&
  a.unlockedAt === b.unlockedAt &&
  a.gameName === b.gameName &&
  a.iconUrl === b.iconUrl;

/** The slot for `item` under `key`: last refresh's if the item is unchanged, else a new one. */
function slotFor(
  next: Map<string, Rendered>,
  key: string,
  item: ShelfItem,
  render: () => HTMLElement,
): HTMLElement {
  const previous = rendered.get(key);
  let slot: HTMLElement;
  if (previous && sameItem(previous.item, item)) {
    slot = previous.slot;
    itemOf.set(slot, item);
    if (trophyOf.has(slot)) trophyOf.set(slot, item);
    // Only a slot made by this refresh drops in; a reused one must not replay it.
    slot.firstElementChild!.classList.remove("new");
  } else {
    slot = render();
  }
  next.set(key, { item, slot });
  return slot;
}

type RowContent = HTMLElement[] | string;
interface RowSpec {
  content: RowContent;
  trophy: boolean;
  /** The nameplate on the rail of a section's first row. */
  label?: string;
}

const kindOf = (content: RowContent) => (typeof content === "string" ? `m:${content}` : "items");
const rowKey = ({ trophy, content, label }: RowSpec) => `${trophy ? "t" : "r"}|${label ?? ""}|${kindOf(content)}`;

function renderRow(spec: RowSpec): HTMLElement {
  const { content, trophy, label } = spec;
  const row = el("div", trophy ? "row trophy" : "row");
  const items = el("div", typeof content === "string" ? "row-items message" : "row-items");
  if (typeof content === "string") items.append(el("span", "placard", content));
  row.dataset.key = rowKey(spec);
  const rail = el("div", "rail");
  if (label) rail.append(el("span", "rail-label", label));
  row.append(items, rail);
  return row;
}

/**
 * Shows `rows`, reusing the row elements already on the shelf where the kinds match. When a
 * level is added or removed, the rows that stay slide to their new places and a new one rises
 * in, rather than everything jumping.
 */
function syncRows(rows: RowSpec[]) {
  const current = [...shelvesEl.children] as HTMLElement[];
  const before = new Map(current.map((row) => [row, row.getBoundingClientRect().top]));
  const pool = new Map<string, HTMLElement[]>();
  for (const row of current) {
    const key = row.dataset.key ?? "";
    pool.set(key, [...(pool.get(key) ?? []), row]);
  }
  const entering = new Set<HTMLElement>();
  const next = rows.map((spec) => {
    const { content } = spec;
    let row = pool.get(rowKey(spec))?.shift();
    if (!row) {
      row = renderRow(spec);
      entering.add(row);
    }
    if (typeof content !== "string") setChildren(row.querySelector(".row-items")!, content);
    return row;
  });
  setChildren(shelvesEl, next);
  if (reducedMotion() || before.size === 0) return;
  for (const row of next) {
    row.getAnimations().forEach((a) => a.cancel());
    if (entering.has(row)) {
      row.animate([{ opacity: 0, transform: "translateY(10px)" }, { opacity: 1, transform: "none" }], {
        duration: 220,
        easing: LIFT_EASING,
      });
      continue;
    }
    const dy = before.get(row)! - row.getBoundingClientRect().top;
    if (dy) {
      row.animate([{ transform: `translateY(${dy}px)` }, { transform: "none" }], { duration: 220, easing: LIFT_EASING });
    }
  }
}

function renderMessage(text: string) {
  rendered = new Map();
  syncRows([{ content: text, trophy: false }]);
}

/**
 * Tells the backend where the shelf is, in physical pixels, so clicks elsewhere pass through
 * to the desktop. The device pixel ratio includes the shelf's zoom. While a slab is carried the
 * whole window takes the mouse: the pointer has to get `DRAG_OUT_MARGIN` past the shelf to drag
 * it out, and a window letting the mouse through there would stop the drag's pointer events.
 */
function reportHitRect() {
  const r = drag?.ghost
    ? { left: 0, top: 0, width: window.innerWidth, height: window.innerHeight }
    : shelvesEl.getBoundingClientRect();
  const px = window.devicePixelRatio;
  invoke("set_hit_rect", { rect: { x: r.left * px, y: r.top * px, width: r.width * px, height: r.height * px } }).catch(
    console.error,
  );
}

/** The last shelf drawn, so a level change can be laid out before the backend sends a new one. */
let lastShelf: Shelf | null = null;
/**
 * The oldest recent unlock shown last time. A recent achievement not seen before only drops in
 * as a new unlock if it is at least this new; older ones are backfill from an added level.
 */
let oldestShown = -Infinity;

function renderShelf(shelf: Shelf) {
  banners = shelf.banners;
  perRow = shelf.perRow;
  trophyFull = shelf.trophyFull;
  document.documentElement.style.setProperty("--per-row", String(perRow));
  const next = new Map<string, Rendered>();
  const rows: RowSpec[] = [];
  const noTrophies = shelf.trophies.every((level) => level.length === 0);
  for (let r = 0; r < shelf.trophyRows; r++) {
    const level = (shelf.trophies[r] ?? []).slice(0, shelf.perRow);
    rows.push({
      trophy: true,
      label: r === 0 ? "Trophies" : undefined,
      content:
        r === 0 && noTrophies
          ? "Pick your trophies from the tray menu"
          : level.map((item) => slotFor(next, renderedKey(item, true), item, () => renderMovable(item, false, true))),
    });
  }
  if (shelf.rows > 0 && shelf.items.length === 0) {
    rows.push({ trophy: false, label: "Recent", content: "No unlocked achievements found yet" });
  } else {
    const isNew = (item: ShelfItem) =>
      knownKeys !== null && !knownKeys.has(keyOf(item)) && item.unlockedAt >= oldestShown;
    for (let r = 0; r < shelf.rows; r++) {
      const slice = shelf.items.slice(r * shelf.perRow, (r + 1) * shelf.perRow);
      rows.push({
        trophy: false,
        label: r === 0 ? "Recent" : undefined,
        content: slice.map((item) =>
          slotFor(next, renderedKey(item, false), item, () => renderMovable(item, isNew(item), false)),
        ),
      });
    }
  }
  syncRows(rows);
  rendered = next;
  lastShelf = shelf;
  pruneShares();

  // Keep the tooltip while its slot is still on the shelf; rows may have moved, so re-place it.
  const anchor = tooltipAnchor;
  if (anchor?.isConnected && (anchor === pinned || anchor.matches(":hover"))) showTooltip(itemOf.get(anchor)!, anchor);
  else hideTooltip();
  requestAnimationFrame(reportHitRect);
}

let refreshSeq = 0;

async function refresh() {
  // A refresh would pull the carried trophy out from under the pointer; it runs after the drop.
  if (drag) {
    pendingRefresh = true;
    return;
  }
  const seq = ++refreshSeq;
  let shelf: Shelf;
  try {
    shelf = await invoke<Shelf>("get_shelf");
  } catch (err) {
    if (seq !== refreshSeq) return;
    hideTooltip();
    lastShelf = null;
    renderMessage(String(err));
    reportHitRect();
    return;
  }
  // A newer refresh or layout started while this one waited; it wins.
  if (seq !== refreshSeq) return;
  if (drag) {
    pendingRefresh = true;
    return;
  }
  renderShelf(shelf);
  knownKeys = new Set(shelf.items.map(keyOf));
  if (shelf.rows === 0) {
    // No recent levels: whatever a level added later shows is backfill, not a new unlock.
    oldestShown = Infinity;
  } else if (shelf.items.length === 0) {
    // Nothing unlocked yet: the first unlock is new.
    oldestShown = -Infinity;
  } else {
    oldestShown = Math.min(...shelf.items.map((item) => item.unlockedAt));
  }
}

/*
 * A level was added or removed, or the rows got longer or shorter. The shelf lays the levels out
 * at once with what it already has (new recent slots stay empty a moment), and the full refresh that follows fills it in.
 */
listen<{ rows: number; trophyRows: number; perRow: number }>("shelf-layout", ({ payload }) => {
  if (!lastShelf || drag) return;
  // The new counts outdate any refresh still on its way.
  refreshSeq++;
  renderShelf({ ...lastShelf, rows: payload.rows, trophyRows: payload.trophyRows, perRow: payload.perRow });
});

window.addEventListener("resize", () => requestAnimationFrame(reportHitRect));
listen("achievements-updated", () => void refresh());
void refresh();
