import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface PickerItem {
  appId: number;
  apiName: string;
  title: string;
  description: string;
  icon: string;
  unlockedAt: number;
  gameName: string;
}

interface PickerData {
  maxTrophies: number;
  trophies: TrophyKey[];
  items: PickerItem[];
}

interface TrophyKey {
  appId: number;
  apiName: string;
}

const listEl = document.getElementById("list") as HTMLElement;
const countEl = document.getElementById("count") as HTMLSpanElement;
const statusEl = document.getElementById("status") as HTMLParagraphElement;
const searchEl = document.getElementById("search") as HTMLInputElement;
const clearEl = document.getElementById("clear") as HTMLButtonElement;
const gamesEl = document.getElementById("games") as HTMLElement;
const gameFilterEl = document.getElementById("game-filter") as HTMLInputElement;

/** Sidebar key for the "All games" entry. */
const ALL_GAMES = 0;

let maxTrophies = 6;
let items: PickerItem[] = [];
let trophies: TrophyKey[] = [];
let chosen = new Set<string>();
/** The game picked in the sidebar, or ALL_GAMES. */
let selectedGame = ALL_GAMES;
/** Cards by achievement key, so toggling only updates classes instead of re-rendering. */
const cards = new Map<string, HTMLButtonElement>();
/** Sidebar buttons by app id (ALL_GAMES for the first entry). */
const gameButtons = new Map<number, HTMLButtonElement>();
let statusTimer: number | undefined;

const keyOf = (item: TrophyKey) => `${item.appId}:${item.apiName}`;

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

function updateChosen(keys: TrophyKey[]) {
  trophies = keys;
  chosen = new Set(keys.map(keyOf));
  for (const [key, card] of cards) {
    const on = chosen.has(key);
    card.classList.toggle("chosen", on);
    card.setAttribute("aria-pressed", String(on));
  }
  const gamesWithTrophies = new Set(keys.map((k) => k.appId));
  for (const [appId, button] of gameButtons) {
    button.classList.toggle("has-trophy", appId !== ALL_GAMES && gamesWithTrophies.has(appId));
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
      invoke<string | null>("get_icon", { appId: Number(img.dataset.appId), icon: img.dataset.icon })
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
      key: { appId: item.appId, apiName: item.apiName },
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
  img.dataset.appId = String(item.appId);
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
function groupByGame(list: PickerItem[]): Map<number, PickerItem[]> {
  const games = new Map<number, PickerItem[]>();
  for (const item of list) {
    const group = games.get(item.appId);
    if (group) group.push(item);
    else games.set(item.appId, [item]);
  }
  return games;
}

function selectGame(appId: number) {
  selectedGame = appId;
  for (const [id, button] of gameButtons) {
    button.classList.toggle("selected", id === appId);
    button.setAttribute("aria-current", String(id === appId));
  }
  render();
  listEl.scrollTop = 0;
}

function gameButton(appId: number, name: string, count: number): HTMLButtonElement {
  const button = el("button", appId === selectedGame ? "game-button selected" : "game-button");
  button.type = "button";
  button.title = name;
  button.setAttribute("aria-current", String(appId === selectedGame));
  button.append(el("span", "game-button-name", name), el("span", "game-button-star", "★"), el("span", "game-button-count", String(count)));
  button.addEventListener("click", () => selectGame(appId));
  gameButtons.set(appId, button);
  return button;
}

function renderSidebar() {
  gameButtons.clear();
  const filter = gameFilterEl.value.trim().toLowerCase();
  const buttons = [gameButton(ALL_GAMES, "All games", items.length)];
  for (const [appId, group] of groupByGame(items)) {
    // Keep the selected game visible even when the filter would hide it.
    if (filter && appId !== selectedGame && !group[0].gameName.toLowerCase().includes(filter)) continue;
    buttons.push(gameButton(appId, group[0].gameName, group.length));
  }
  gamesEl.replaceChildren(...buttons);
  updateChosen(trophies);
}

function render() {
  iconObserver.disconnect();
  cards.clear();
  const query = searchEl.value.trim().toLowerCase();
  const matches = items.filter(
    (i) =>
      (selectedGame === ALL_GAMES || i.appId === selectedGame) &&
      (!query || `${i.title} ${i.description} ${i.gameName}`.toLowerCase().includes(query)),
  );

  if (matches.length === 0) {
    listEl.replaceChildren(el("p", "empty", items.length ? "Nothing matches your search" : "No unlocked achievements found yet"));
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
  items = data.items;
  trophies = data.trophies;
  // Choosing a trophy also fires an update; only rebuild the lists when the achievements changed.
  if (changed || cards.size === 0) {
    if (!items.some((i) => i.appId === selectedGame)) selectedGame = ALL_GAMES;
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
