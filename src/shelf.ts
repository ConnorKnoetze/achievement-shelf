import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { rarityTier } from "./rarity";

interface ShelfItem {
  source: "steam" | "gog";
  gameId: string;
  apiName: string;
  title: string;
  description: string;
  hidden: boolean;
  icon: string;
  unlockedAt: number;
  rarity: number | null;
  gameName: string;
  iconUrl: string | null;
}

interface Shelf {
  /** Rows of recent achievements; may be 0. */
  rows: number;
  /** Rows of the trophy shelf; at least 1. */
  trophyRows: number;
  perRow: number;
  trophies: ShelfItem[];
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

function showTooltip(item: ShelfItem, anchor: HTMLElement) {
  tooltipAnchor = anchor;
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
  children.push(certificateGrade(item));

  tooltipEl.classList.toggle("has-banner", Boolean(banner));
  tooltipEl.replaceChildren(...children);
  tooltipEl.hidden = false;

  // The slot never moves on hover (only the slab inside it lifts), so this is stable.
  const box = anchor.getBoundingClientRect();
  const width = tooltipEl.offsetWidth;
  const left = Math.min(Math.max(8, box.left + box.width / 2 - width / 2), window.innerWidth - width - 8);
  tooltipEl.style.left = `${left}px`;
  tooltipEl.style.top = `${Math.max(4, box.top - tooltipEl.offsetHeight - 10)}px`;
}

function hideTooltip() {
  tooltipAnchor = null;
  tooltipEl.hidden = true;
}

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
  slab.append(gradeLabel(item, "grade slab-grade"), iconInsert(item, "slab-insert", item.title));
  slot.append(slab);
  itemOf.set(slot, item);
  slot.addEventListener("mouseenter", () => {
    if (!drag?.ghost) showTooltip(itemOf.get(slot)!, slot);
  });
  slot.addEventListener("mouseleave", hideTooltip);
  return slot;
}

/** Replaces `parent`'s children with `nodes`, leaving it untouched when they already match. */
function setChildren(parent: Element, nodes: HTMLElement[]) {
  const current = parent.children;
  if (current.length === nodes.length && nodes.every((node, i) => current[i] === node)) return;
  parent.replaceChildren(...nodes);
}

/*
 * Trophies are dragged to reorder them, across trophy rows too. The pointer is captured on
 * the shelves element, not the slot, because the slot moves between rows while it is dragged.
 * The slot stays in place as a faded placeholder and a copy of its slab follows the pointer.
 *
 * Rows always hold `perRow` trophies in turn, so the place of slot k does not depend on the
 * order. Those places are measured once when the drag starts; measuring live would read slots
 * mid-slide and flip the drop target back and forth under a still pointer.
 */
interface Drag {
  slot: HTMLElement;
  pointerId: number;
  startX: number;
  startY: number;
  offsetX: number;
  offsetY: number;
  ghost: HTMLElement | null;
  initial: HTMLElement[];
  order: HTMLElement[];
  /** Centre of each trophy place, in `.shelves` coordinates. */
  places: { x: number; y: number }[];
  /** Latest pointer position, drawn on the next frame. */
  pointerX: number;
  pointerY: number;
  frame: number;
}

const DRAG_THRESHOLD = 4;
/** How much closer (px) another place must be before the drop target moves to it. */
const DROP_HYSTERESIS = 6;
const trophyOf = new WeakMap<HTMLElement, ShelfItem>();
let perRow = 6;
let drag: Drag | null = null;
let pendingRefresh = false;

const trophySlots = () => [...shelvesEl.querySelectorAll<HTMLElement>(".row.trophy .slot")];

/** Top-left of `slot`'s place in client coordinates, ignoring transforms and animations. */
function placeOf(slot: HTMLElement) {
  const origin = shelvesEl.getBoundingClientRect();
  return { left: origin.left + slot.offsetLeft, top: origin.top + slot.offsetTop };
}

/** Puts the trophy slots in `order`, filling the trophy rows in turn; others slide over. */
function layoutTrophies(order: HTMLElement[]) {
  const before = new Map(order.map((slot) => [slot, slot.getBoundingClientRect()]));
  for (const slot of order) slot.getAnimations().forEach((a) => a.cancel());
  shelvesEl.querySelectorAll(".row.trophy .row-items").forEach((row, r) => {
    setChildren(row, order.slice(r * perRow, (r + 1) * perRow));
  });
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

/** Where the dragged slot would go: the place nearest the pointer, with some hysteresis. */
function dropOrder(d: Drag, x: number, y: number): HTMLElement[] {
  const origin = shelvesEl.getBoundingClientRect();
  const px = x - origin.left;
  const py = y - origin.top;
  const distance = (i: number) => Math.hypot(px - d.places[i].x, py - d.places[i].y);
  const current = d.order.indexOf(d.slot);
  let target = current;
  let best = distance(current);
  d.places.forEach((_, i) => {
    const dist = distance(i);
    if (dist < best) {
      best = dist;
      target = i;
    }
  });
  if (target === current || best + DROP_HYSTERESIS > distance(current)) return d.order;
  const others = d.order.filter((slot) => slot !== d.slot);
  others.splice(target, 0, d.slot);
  return others;
}

const ghostTransform = (x: number, y: number, tilt = true) =>
  `translate3d(${x}px, ${y}px, 0)${tilt ? " rotate(-3deg) scale(1.05)" : ""}`;

function startDrag(event: PointerEvent, slot: HTMLElement) {
  if (event.button !== 0 || drag) return;
  const box = slot.firstElementChild!.getBoundingClientRect();
  const order = trophySlots();
  drag = {
    slot,
    pointerId: event.pointerId,
    startX: event.clientX,
    startY: event.clientY,
    offsetX: event.clientX - box.left,
    offsetY: event.clientY - box.top,
    ghost: null,
    initial: order,
    order,
    places: order.map((s) => ({ x: s.offsetLeft + s.offsetWidth / 2, y: s.offsetTop + s.offsetHeight / 2 })),
    pointerX: event.clientX,
    pointerY: event.clientY,
    frame: 0,
  };
  shelvesEl.setPointerCapture(event.pointerId);
}

/** Glides the ghost into the slot's place, then shows the slot again. */
function settleGhost(ghost: HTMLElement, slot: HTMLElement) {
  const finish = () => {
    ghost.remove();
    slot.classList.remove("drag-source");
  };
  if (reducedMotion() || !slot.isConnected) return finish();
  const { left, top } = placeOf(slot);
  ghost
    .animate([{ transform: ghost.style.transform }, { transform: ghostTransform(left, top, false) }], {
      duration: 180,
      easing: LIFT_EASING,
      fill: "forwards",
    })
    .finished.then(finish, finish);
}

/** Ends the drag; saves the new order when `save` and it changed, otherwise puts it back. */
function endDrag(save: boolean) {
  const d = drag;
  if (!d) return;
  drag = null;
  cancelAnimationFrame(d.frame);
  if (shelvesEl.hasPointerCapture(d.pointerId)) shelvesEl.releasePointerCapture(d.pointerId);
  let saving = false;
  if (d.ghost) {
    shelvesEl.classList.remove("dragging");
    const changed = d.order.some((slot, i) => slot !== d.initial[i]);
    if (changed && !save) layoutTrophies(d.initial);
    settleGhost(d.ghost, d.slot);
    if (changed && save) {
      saving = true;
      const order = d.order.map((slot) => {
        const { source, gameId, apiName } = trophyOf.get(slot)!;
        return { source, gameId, apiName };
      });
      invoke("reorder_trophies", { order }).catch((err) => {
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

function drawDrag() {
  const d = drag;
  if (!d?.ghost) return;
  d.frame = 0;
  d.ghost.style.transform = ghostTransform(d.pointerX - d.offsetX, d.pointerY - d.offsetY);
  const order = dropOrder(d, d.pointerX, d.pointerY);
  if (order !== d.order) {
    d.order = order;
    layoutTrophies(order);
  }
}

shelvesEl.addEventListener("pointermove", (event) => {
  const d = drag;
  if (!d || event.pointerId !== d.pointerId) return;
  // The button came up while the window was ignoring the mouse (outside the shelf).
  if (event.buttons === 0) return endDrag(true);
  if (!d.ghost) {
    if (Math.hypot(event.clientX - d.startX, event.clientY - d.startY) < DRAG_THRESHOLD) return;
    hideTooltip();
    const ghost = d.slot.firstElementChild!.cloneNode(true) as HTMLElement;
    ghost.classList.remove("new");
    ghost.classList.add("drag-ghost");
    ghost.style.transform = ghostTransform(event.clientX - d.offsetX, event.clientY - d.offsetY);
    document.body.append(ghost);
    d.ghost = ghost;
    d.slot.classList.add("drag-source");
    shelvesEl.classList.add("dragging");
  }
  d.pointerX = event.clientX;
  d.pointerY = event.clientY;
  if (!d.frame) d.frame = requestAnimationFrame(drawDrag);
});
shelvesEl.addEventListener("pointerup", (event) => {
  if (drag && event.pointerId === drag.pointerId) endDrag(true);
});
shelvesEl.addEventListener("pointercancel", () => endDrag(false));
shelvesEl.addEventListener("lostpointercapture", () => endDrag(false));

function renderTrophy(item: ShelfItem): HTMLElement {
  const slot = renderItem(item, false);
  slot.classList.add("movable");
  trophyOf.set(slot, item);
  slot.addEventListener("pointerdown", (event) => startDrag(event, slot));
  return slot;
}

/*
 * Refreshes reuse the slots of unchanged achievements, so their icons are not decoded again,
 * the hovered slab stays lifted and the tooltip stays up. Updates come often: every trophy
 * reorder, row change and stats file write by Steam or GOG.
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
}

const kindOf = (content: RowContent) => (typeof content === "string" ? `m:${content}` : "items");
const rowKey = (trophy: boolean, content: RowContent) => `${trophy ? "t" : "r"}|${kindOf(content)}`;

function renderRow(content: RowContent, trophy: boolean): HTMLElement {
  const row = el("div", trophy ? "row trophy" : "row");
  const items = el("div", typeof content === "string" ? "row-items message" : "row-items");
  if (typeof content === "string") items.append(el("span", "placard", content));
  row.dataset.key = rowKey(trophy, content);
  row.append(items, el("div", "rail"));
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
  const next = rows.map(({ content, trophy }) => {
    let row = pool.get(rowKey(trophy, content))?.shift();
    if (!row) {
      row = renderRow(content, trophy);
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
 * to the desktop. The device pixel ratio includes the shelf's zoom.
 */
function reportHitRect() {
  const r = shelvesEl.getBoundingClientRect();
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
  const next = new Map<string, Rendered>();
  const rows: RowSpec[] = [];
  for (let r = 0; r < shelf.trophyRows; r++) {
    const slice = shelf.trophies.slice(r * shelf.perRow, (r + 1) * shelf.perRow);
    rows.push({
      trophy: true,
      content:
        r === 0 && shelf.trophies.length === 0
          ? "Pick your trophies from the tray menu"
          : slice.map((item) => slotFor(next, `t:${keyOf(item)}`, item, () => renderTrophy(item))),
    });
  }
  if (shelf.rows > 0 && shelf.items.length === 0) {
    rows.push({ trophy: false, content: "No unlocked achievements found yet" });
  } else {
    const isNew = (item: ShelfItem) =>
      knownKeys !== null && !knownKeys.has(keyOf(item)) && item.unlockedAt >= oldestShown;
    for (let r = 0; r < shelf.rows; r++) {
      const slice = shelf.items.slice(r * shelf.perRow, (r + 1) * shelf.perRow);
      rows.push({
        trophy: false,
        content: slice.map((item) => slotFor(next, `r:${keyOf(item)}`, item, () => renderItem(item, isNew(item)))),
      });
    }
  }
  syncRows(rows);
  rendered = next;
  lastShelf = shelf;

  // Keep the tooltip while its slot is still on the shelf; rows may have moved, so re-place it.
  const anchor = tooltipAnchor;
  if (anchor?.isConnected && anchor.matches(":hover")) showTooltip(itemOf.get(anchor)!, anchor);
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
 * A level was added or removed. The shelf lays the levels out at once with what it already has
 * (a new recent level stays empty a moment), and the full refresh that follows fills it in.
 */
listen<{ rows: number; trophyRows: number }>("shelf-layout", ({ payload }) => {
  if (!lastShelf || drag) return;
  // The new counts outdate any refresh still on its way.
  refreshSeq++;
  renderShelf({ ...lastShelf, rows: payload.rows, trophyRows: payload.trophyRows });
});

window.addEventListener("resize", () => requestAnimationFrame(reportHitRect));
listen("achievements-updated", () => void refresh());
void refresh();
