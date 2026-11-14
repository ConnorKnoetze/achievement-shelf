import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Source = "steam" | "gog";

interface PickerItem {
  source: Source;
  gameId: string;
  apiName: string;
  title: string;
  description: string;
  icon: string;
  unlockedAt: number;
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

const listEl = document.getElementById("list") as HTMLElement;
const countEl = document.getElementById("count") as HTMLSpanElement;
const statusEl = document.getElementById("status") as HTMLParagraphElement;
const searchEl = document.getElementById("search") as HTMLInputElement;
const clearEl = document.getElementById("clear") as HTMLButtonElement;
const tabsEl = document.getElementById("tabs") as HTMLElement;
const gamesEl = document.getElementById("games") as HTMLElement;
const gameFilterEl = document.getElementById("game-filter") as HTMLInputElement;

/** Sidebar key for the "All games" entry. */
const ALL_GAMES = "";

let maxTrophies = 6;
let launchers: Source[] = [];
let items: PickerItem[] = [];
let trophies: TrophyKey[] = [];
let chosen = new Set<string>();
/** The launcher tab shown, or null until the first load picks one. */
let selectedSource: Source | null = null;
/** The game picked in the sidebar (a game key), or ALL_GAMES. */
let selectedGame = ALL_GAMES;
/** Cards by achievement key, so toggling only updates classes instead of re-rendering. */
const cards = new Map<string, HTMLButtonElement>();
/** Sidebar buttons by game key (ALL_GAMES for the first entry). */
const gameButtons = new Map<string, HTMLButtonElement>();
const tabButtons = new Map<Source, HTMLButtonElement>();
let statusTimer: number | undefined;

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

function storedTab(): Source | null {
  try {
    const value = localStorage.getItem(TAB_STORAGE_KEY);
    return TABS.some((t) => t.source === value) ? (value as Source) : null;
  } catch {
    return null;
  }
}

function storeTab(source: Source) {
  try {
    localStorage.setItem(TAB_STORAGE_KEY, source);
  } catch {
    // Remembering the tab is only a convenience.
  }
}

/** Items of the selected launcher; the sidebar, list, and search only cover these. */
function tabItems(): PickerItem[] {
  return items.filter((i) => i.source === selectedSource);
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
  countEl.textContent = `${chosen.size} / ${maxTrophies} chosen`;
  clearEl.disabled = chosen.size === 0;
}

/** Icons are loaded only when a card scrolls into view; a library can hold thousands. */
const iconObserver = new IntersectionObserver(
  (entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      const img = entry.target as HTMLImageElement;
      iconObserver.unobserve(img);
      const { source, gameId, icon } = img.dataset;
      invoke<string | null>("get_icon", { source, gameId, icon })
        .then((url) => {
          if (url) img.src = url;
          else img.classList.add("missing");
        })
        .catch(() => img.classList.add("missing"));
    }
  },
  { root: listEl, rootMargin: "300px" },
);

async function toggle(item: PickerItem) {
  const trophy = !chosen.has(keyOf(item));
  try {
    const keys = await invoke<TrophyKey[]>("set_trophy", {
      key: { source: item.source, gameId: item.gameId, apiName: item.apiName },
      trophy,
    });
    updateChosen(keys);
  } catch (err) {
    showStatus(String(err));
  }
}

function renderCard(item: PickerItem): HTMLButtonElement {
  const card = el("button", "card");
  card.type = "button";
  const img = el("img", "card-icon");
  img.alt = "";
  img.draggable = false;
  img.dataset.source = item.source;
  img.dataset.gameId = item.gameId;
  img.dataset.icon = item.icon;
  iconObserver.observe(img);

  const text = el("div", "card-text");
  text.append(
    el("div", "card-title", item.title || item.apiName),
    el("div", "card-desc", item.description || "No description"),
    el("div", "card-date", formatDate(item.unlockedAt)),
  );
  card.append(img, text, el("span", "card-badge", "★"));
  card.addEventListener("click", () => void toggle(item));
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

function selectTab(source: Source, focus = false) {
  if (source !== selectedSource) {
    selectedSource = source;
    selectedGame = ALL_GAMES;
    gameFilterEl.value = "";
    storeTab(source);
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
    button.append(el("span", "tab-label", label), el("span", "tab-star", "★"), el("span", "tab-count", String(count)));
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
  button.append(el("span", "game-button-name", name), el("span", "game-button-star", "★"), el("span", "game-button-count", String(count)));
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
  if (list.length) return "Nothing matches your search";
  const tab = TABS.find((t) => t.source === selectedSource);
  if (tab && !launchers.includes(tab.source)) return tab.missing;
  return "No unlocked achievements found yet";
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

  const games = groupByGame(matches);
  const sections: HTMLElement[] = [];
  for (const group of games.values()) {
    const section = el("section", "game");
    const heading = el("h2", "game-name", group[0].gameName);
    heading.append(el("span", "game-count", String(group.length)));
    const grid = el("div", "grid");
    for (const item of group) {
      const card = renderCard(item);
      cards.set(keyOf(item), card);
      grid.append(card);
    }
    section.append(heading, grid);
    sections.push(section);
  }
  listEl.replaceChildren(...sections);
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

searchEl.addEventListener("input", render);
gameFilterEl.addEventListener("input", renderSidebar);
clearEl.addEventListener("click", () => {
  invoke("clear_trophies")
    .then(() => updateChosen([]))
    .catch((err) => showStatus(String(err)));
});
listen("achievements-updated", () => void load());
void load();
