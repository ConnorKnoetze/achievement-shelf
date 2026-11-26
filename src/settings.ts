import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface SettingsView {
  rows: number;
  trophyRows: number;
  maxLevels: number;
  display: string;
  displays: { name: string; label: string }[];
  scale: number;
  minScale: number;
  maxScale: number;
  canAddRow: boolean;
  canRemoveRecentRow: boolean;
  canRemoveTrophyRow: boolean;
  onTop: boolean;
  autostart: boolean;
}

const byId = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const totalEl = byId<HTMLSpanElement>("levels-total");
const trophyRowsEl = byId<HTMLOutputElement>("trophy-rows");
const recentRowsEl = byId<HTMLOutputElement>("recent-rows");
const trophyAddEl = byId<HTMLButtonElement>("trophy-add");
const trophyRemoveEl = byId<HTMLButtonElement>("trophy-remove");
const recentAddEl = byId<HTMLButtonElement>("recent-add");
const recentRemoveEl = byId<HTMLButtonElement>("recent-remove");
const displayEl = byId<HTMLSelectElement>("display");
const scaleEl = byId<HTMLInputElement>("scale");
const scaleValueEl = byId<HTMLOutputElement>("scale-value");
const onTopEl = byId<HTMLInputElement>("on-top");
const autostartEl = byId<HTMLInputElement>("autostart");
const statusEl = byId<HTMLParagraphElement>("status");
const diagramEl = byId<HTMLDivElement>("levels-diagram");
const versionEl = byId<HTMLSpanElement>("version");

/** Slabs per level; matches PER_ROW in src-tauri/src/settings.rs. */
const PER_ROW = 6;
/** Levels drawn last time, so an added level can drop into place. */
let drawn: { trophy: number; recent: number } | null = null;

let statusTimer: number | undefined;

/** Whether the scale slider is held down. */
let draggingScale = false;
/** Whether a resize is on its way to the shelf, and the newest scale to send after it. */
let sendingScale = false;
let queuedScale: number | null = null;

function showStatus(text: string) {
  statusEl.textContent = text;
  statusEl.hidden = false;
  window.clearTimeout(statusTimer);
  statusTimer = window.setTimeout(() => (statusEl.hidden = true), 5000);
}

function showScale(scale: number) {
  scaleValueEl.textContent = `${Math.round(scale * 100)}%`;
  const min = Number(scaleEl.min);
  const max = Number(scaleEl.max);
  const fill = max > min ? ((scale - min) / (max - min)) * 100 : 0;
  scaleEl.style.setProperty("--fill", `${fill}%`);
}

function level(trophy: boolean, isNew: boolean): HTMLElement {
  const row = document.createElement("div");
  row.className = `dlevel${trophy ? " trophy" : ""}${isNew ? " new" : ""}`;
  const slabs = document.createElement("div");
  slabs.className = "dslabs";
  for (let i = 0; i < PER_ROW; i++) {
    const slab = document.createElement("span");
    slab.className = "dslab acrylic";
    slabs.append(slab);
  }
  const rail = document.createElement("div");
  rail.className = "drail";
  row.append(slabs, rail);
  return row;
}

/** Draws the shelf's levels as the shelf stacks them: trophy rails above recent rails. */
function drawLevels(trophy: number, recent: number) {
  const before = drawn;
  drawn = { trophy, recent };
  if (before && before.trophy === trophy && before.recent === recent) return;
  const rows: HTMLElement[] = [];
  // A new trophy level appears at the bottom of the trophy rails, a new recent level at the top of the recents.
  for (let i = 0; i < trophy; i++) rows.push(level(true, !!before && i >= before.trophy));
  for (let i = 0; i < recent; i++) rows.push(level(false, !!before && recent > before.recent && i === 0));
  diagramEl.replaceChildren(...rows);
}

function render(view: SettingsView) {
  const main = new Option("Main display", "");
  displayEl.replaceChildren(main, ...view.displays.map((d) => new Option(d.label, d.name)));
  displayEl.value = view.display;
  scaleEl.min = String(view.minScale);
  scaleEl.max = String(view.maxScale);
  // While dragging, the thumb stays under the pointer rather than jumping back to a value the
  // shelf has only just caught up to.
  if (!draggingScale) {
    scaleEl.value = String(view.scale);
    showScale(view.scale);
  }
  trophyRowsEl.value = String(view.trophyRows);
  recentRowsEl.value = String(view.rows);
  totalEl.textContent = `${view.rows + view.trophyRows} / ${view.maxLevels}`;
  drawLevels(view.trophyRows, view.rows);
  trophyAddEl.disabled = !view.canAddRow;
  recentAddEl.disabled = !view.canAddRow;
  trophyRemoveEl.disabled = !view.canRemoveTrophyRow;
  recentRemoveEl.disabled = !view.canRemoveRecentRow;
  onTopEl.checked = view.onTop;
  autostartEl.checked = view.autostart;
}

async function call(command: string, args: Record<string, unknown>) {
  try {
    render(await invoke<SettingsView>(command, args));
  } catch (err) {
    showStatus(String(err));
    // Puts the controls back to the real settings after a failed change.
    render(await invoke<SettingsView>("get_settings"));
  }
}

const changeRows = (trophy: boolean, add: boolean) => () => void call("change_shelf_rows", { trophy, add });
trophyAddEl.addEventListener("click", changeRows(true, true));
trophyRemoveEl.addEventListener("click", changeRows(true, false));
recentAddEl.addEventListener("click", changeRows(false, true));
recentRemoveEl.addEventListener("click", changeRows(false, false));
displayEl.addEventListener("change", () => void call("set_shelf_display", { name: displayEl.value }));
/** Resizes the shelf, one resize at a time; scales picked meanwhile collapse to the newest. */
async function sendScale(scale: number) {
  if (sendingScale) {
    queuedScale = scale;
    return;
  }
  sendingScale = true;
  try {
    await call("set_shelf_scale", { scale });
  } finally {
    sendingScale = false;
    if (queuedScale !== null) {
      const next = queuedScale;
      queuedScale = null;
      void sendScale(next);
    }
  }
}

// The shelf resizes as the slider moves, by drag or keyboard.
scaleEl.addEventListener("input", () => {
  showScale(scaleEl.valueAsNumber);
  void sendScale(scaleEl.valueAsNumber);
});
scaleEl.addEventListener("pointerdown", () => (draggingScale = true));
for (const type of ["pointerup", "pointercancel"]) {
  window.addEventListener(type, () => (draggingScale = false));
}
onTopEl.addEventListener("change", () => void call("set_keep_on_top", { value: onTopEl.checked }));
autostartEl.addEventListener("change", () => void call("set_start_with_windows", { value: autostartEl.checked }));

// Changes made from the tray menu.
listen<SettingsView>("settings-changed", (event) => render(event.payload));
invoke<SettingsView>("get_settings").then(render).catch((err) => showStatus(String(err)));
getVersion()
  .then((version) => (versionEl.textContent = `v${version}`))
  .catch(() => versionEl.parentElement?.remove());
