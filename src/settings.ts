import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface SettingsView {
  rows: number;
  trophyRows: number;
  maxLevels: number;
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
const onTopEl = byId<HTMLInputElement>("on-top");
const autostartEl = byId<HTMLInputElement>("autostart");
const statusEl = byId<HTMLParagraphElement>("status");

let statusTimer: number | undefined;

function showStatus(text: string) {
  statusEl.textContent = text;
  statusEl.hidden = false;
  window.clearTimeout(statusTimer);
  statusTimer = window.setTimeout(() => (statusEl.hidden = true), 5000);
}

function render(view: SettingsView) {
  trophyRowsEl.value = String(view.trophyRows);
  recentRowsEl.value = String(view.rows);
  totalEl.textContent = `${view.rows + view.trophyRows} / ${view.maxLevels}`;
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
onTopEl.addEventListener("change", () => void call("set_keep_on_top", { value: onTopEl.checked }));
autostartEl.addEventListener("change", () => void call("set_start_with_windows", { value: autostartEl.checked }));

// Changes made from the tray menu.
listen<SettingsView>("settings-changed", (event) => render(event.payload));
invoke<SettingsView>("get_settings").then(render).catch((err) => showStatus(String(err)));
