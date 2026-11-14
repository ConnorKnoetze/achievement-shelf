import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface ShelfItem {
  appId: number;
  apiName: string;
  title: string;
  description: string;
  hidden: boolean;
  icon: string;
  unlockedAt: number;
  gameName: string;
  iconUrl: string | null;
}

interface Shelf {
  rows: number;
  perRow: number;
  items: ShelfItem[];
}

const shelvesEl = document.getElementById("shelves") as HTMLDivElement;
const tooltipEl = document.getElementById("tooltip") as HTMLDivElement;

let knownKeys: Set<string> | null = null;

const keyOf = (item: ShelfItem) => `${item.appId}:${item.apiName}`;

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
  tooltipEl.replaceChildren(
    el("div", "tooltip-game", item.gameName),
    el("div", "tooltip-title", item.title || item.apiName),
    el("div", "tooltip-desc", item.description || "No description"),
    el("div", "tooltip-date", formatDate(item.unlockedAt)),
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

function renderMessage(text: string) {
  const row = el("div", "row");
  row.append(el("div", "row-items message", text), el("div", "plank"));
  shelvesEl.replaceChildren(row);
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
  if (shelf.items.length === 0) {
    renderMessage("No unlocked achievements found yet");
  } else {
    const rows: HTMLElement[] = [];
    for (let r = 0; r < shelf.rows; r++) {
      const slice = shelf.items.slice(r * shelf.perRow, (r + 1) * shelf.perRow);
      const row = el("div", "row");
      const items = el("div", "row-items");
      for (const item of slice) items.append(renderItem(item, knownKeys !== null && !knownKeys.has(keyOf(item))));
      row.append(items, el("div", "plank"));
      rows.push(row);
    }
    shelvesEl.replaceChildren(...rows);
  }
  knownKeys = new Set(shelf.items.map(keyOf));
  requestAnimationFrame(reportHitRect);
}

window.addEventListener("resize", () => requestAnimationFrame(reportHitRect));
listen("achievements-updated", () => void refresh());
void refresh();
