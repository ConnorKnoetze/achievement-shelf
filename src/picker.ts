import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { gradeLabel } from "./rarity";

type Source = "steam" | "gog";
type Sort = "newest" | "rarest";

interface PickerItem {
  source: Source;
  gameId: string;
  apiName: string;
  title: string;
  description: string;
  icon: string;
  unlockedAt: number;
  rarity: number | null;
  gameName: string;
}

interface PickerData {
  launchers: Source[];
  maxTrophies: number;
  trophies: TrophyKey[];
  items: PickerItem[];
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
/** Slabs per rail; matches PER_ROW in src-tauri/src/settings.rs. */
const PER_RAIL = 6;
const HINT_EMPTY = "Pick the achievements you are most proud of. They sit on the gold shelf above your recent unlocks.";
const HINT_PLACED = "Click a trophy on the rail to take it off the shelf.";

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

/** Sidebar key for the "All games" entry. */
const ALL_GAMES = "";

/** Drawn cup for achievements without an icon, as on the shelf. */
const FALLBACK_ICON =
  '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
  '<path d="M7 4h10v5a5 5 0 0 1-10 0V4z"/><path d="M7 6H4.5a2.5 2.5 0 0 0 2.9 3.9M17 6h2.5a2.5 2.5 0 0 1-2.9 3.9"/>' +
  '<path d="M12 14v4M8.5 20h7"/></svg>';

let maxTrophies = 6;
let launchers: Source[] = [];
let items: PickerItem[] = [];
let itemsByKey = new Map<string, PickerItem>();
let trophies: TrophyKey[] = [];
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

function storedSort(): Sort {
  return readStored(SORT_STORAGE_KEY) === "rarest" ? "rarest" : "newest";
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

/** Draws the trophy rails: placed trophies in order, then empty slots up to the capacity. */
function renderRails() {
  const previous = railKeys;
  const placed = trophies.slice(0, maxTrophies);
  railKeys = new Set(placed.map(keyOf));
  const railCount = Math.max(1, Math.ceil(maxTrophies / PER_RAIL));
  const rails: HTMLElement[] = [];
  for (let r = 0; r < railCount; r++) {
    const slots = el("div", "mini-slots");
    for (let i = r * PER_RAIL; i < (r + 1) * PER_RAIL && i < maxTrophies; i++) {
      const key = placed[i];
      if (!key) {
        slots.append(el("span", "pslot"));
        continue;
      }
      const item = itemsByKey.get(keyOf(key));
      const isNew = previous !== null && !previous.has(keyOf(key));
      const slab = el("button", isNew ? "pslab acrylic new" : "pslab acrylic");
      slab.type = "button";
      const title = item ? item.title || item.apiName : "An achievement no longer found";
      slab.title = `${title}\nClick to take it off the shelf`;
      slab.setAttribute("aria-label", `Remove ${title} from the shelf`);
      slab.append(gradeLabel(item?.rarity ?? null, "grade pslab-grade"), insert(item, false));
      slab.addEventListener("click", () => void setTrophy(key, false));
      slots.append(slab);
    }
    const rail = el("div", "mini-rail");
    rail.setAttribute("role", "listitem");
    rail.append(slots, el("div", "rail trophy"));
    rails.push(rail);
  }
  railsEl.replaceChildren(...rails);
}

function updateChosen(keys: TrophyKey[]) {
  trophies = keys;
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
  countEl.textContent = `${Math.min(chosen.size, maxTrophies)} / ${maxTrophies}`;
  hintEl.textContent = chosen.size ? HINT_PLACED : HINT_EMPTY;
  clearEl.disabled = chosen.size === 0;
  renderRails();
}

async function setTrophy(key: TrophyKey, trophy: boolean) {
  try {
    const keys = await invoke<TrophyKey[]>("set_trophy", {
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
  foot.append(meta, gradeLabel(item.rarity, "grade card-grade"));
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

  if (sort === "rarest") {
    // One ranked list across games, so the rarest pieces in the library come first.
    const section = el("section", "flat");
    section.append(gridOf([...matches].sort(byRarity), selectedGame === ALL_GAMES));
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
  maxTrophies = data.maxTrophies;
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
  sort = sortEl.value === "rarest" ? "rarest" : "newest";
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
listen("achievements-updated", () => void load());
void load();
