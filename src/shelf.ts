import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { rarityLabel } from "./rarity";

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

function showTooltip(item: ShelfItem, anchor: HTMLElement) {
  const date = el("div", "tooltip-date", formatDate(item.unlockedAt));
  const rarity = rarityLabel(item.rarity);
  if (rarity) date.append(" · ", rarity);
  const head: HTMLElement[] = [];
  const banner = banners[gameKeyOf(item)];
  if (banner) {
    const img = el("img", "tooltip-banner");
    img.src = banner;
    img.alt = "";
    head.push(img);
  }
  tooltipEl.classList.toggle("has-banner", head.length > 0);
  tooltipEl.replaceChildren(
    ...head,
    el("div", "tooltip-game", item.gameName),
    el("div", "tooltip-title", item.title || item.apiName),
    el("div", "tooltip-desc", item.description || "No description"),
    date,
  );
  tooltipEl.hidden = false;

  const box = anchor.getBoundingClientRect();
  const width = tooltipEl.offsetWidth;
  const left = Math.min(Math.max(8, box.left + box.width / 2 - width / 2), window.innerWidth - width - 8);
  tooltipEl.style.left = `${left}px`;
  tooltipEl.style.top = `${Math.max(4, box.top - tooltipEl.offsetHeight - 10)}px`;
}

function hideTooltip() {
  tooltipEl.hidden = true;
}

function renderItem(item: ShelfItem, isNew: boolean): HTMLElement {
  const plaque = el("div", isNew ? "item new" : "item");
  if (item.iconUrl) {
    const img = document.createElement("img");
    img.src = item.iconUrl;
    img.alt = item.title;
    img.draggable = false;
    plaque.append(img);
  } else {
    plaque.append(el("div", "item-fallback", "🏆"));
  }
  plaque.addEventListener("mouseenter", () => showTooltip(item, plaque));
  plaque.addEventListener("mouseleave", hideTooltip);
  return plaque;
}

function renderRow(content: HTMLElement[] | string, trophy = false): HTMLElement {
  const row = el("div", trophy ? "row trophy" : "row");
  const items = el("div", typeof content === "string" ? "row-items message" : "row-items");
  if (typeof content === "string") items.textContent = content;
  else items.append(...content);
  row.append(items, el("div", "plank"));
  return row;
}

function renderMessage(text: string) {
  shelvesEl.replaceChildren(renderRow(text));
}

/** Tells the backend where the shelf is, so clicks elsewhere pass through to the desktop. */
function reportHitRect() {
  const r = shelvesEl.getBoundingClientRect();
  invoke("set_hit_rect", { rect: { x: r.left, y: r.top, width: r.width, height: r.height } }).catch(console.error);
}

async function refresh() {
  let shelf: Shelf;
  try {
    shelf = await invoke<Shelf>("get_shelf");
  } catch (err) {
    renderMessage(String(err));
    reportHitRect();
    return;
  }

  hideTooltip();
  banners = shelf.banners;
  const rows: HTMLElement[] = [];
  for (let r = 0; r < shelf.trophyRows; r++) {
    const slice = shelf.trophies.slice(r * shelf.perRow, (r + 1) * shelf.perRow);
    rows.push(
      r === 0 && shelf.trophies.length === 0
        ? renderRow("Pick your trophies from the tray menu", true)
        : renderRow(slice.map((item) => renderItem(item, false)), true),
    );
  }
  if (shelf.rows > 0 && shelf.items.length === 0) {
    rows.push(renderRow("No unlocked achievements found yet"));
  } else {
    for (let r = 0; r < shelf.rows; r++) {
      const slice = shelf.items.slice(r * shelf.perRow, (r + 1) * shelf.perRow);
      rows.push(renderRow(slice.map((item) => renderItem(item, knownKeys !== null && !knownKeys.has(keyOf(item))))));
    }
  }
  shelvesEl.replaceChildren(...rows);
  knownKeys = new Set(shelf.items.map(keyOf));
  requestAnimationFrame(reportHitRect);
}

window.addEventListener("resize", () => requestAnimationFrame(reportHitRect));
listen("achievements-updated", () => void refresh());
void refresh();
