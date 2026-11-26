import { listen } from "@tauri-apps/api/event";

/**
 * Matches `UpdateStatus` in src-tauri/src/lib.rs. Before the first one arrives, the page
 * shows the update check as laid out in updater.html.
 */
type UpdateStatus =
  | { state: "downloading"; version: string; downloaded: number; total: number | null }
  | { state: "installing" }
  | { state: "failed" };

const byId = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const plateEl = document.querySelector<HTMLElement>(".plate")!;
const titleEl = byId<HTMLHeadingElement>("title");
const versionEl = byId<HTMLSpanElement>("version");
const messageEl = byId<HTMLParagraphElement>("message");
const progressEl = byId<HTMLDivElement>("progress");
const amountEl = byId<HTMLSpanElement>("amount");
const percentEl = byId<HTMLSpanElement>("percent");

const megabytes = (bytes: number) => (bytes / 1_048_576).toFixed(1);

function setDone(fraction: number | null) {
  progressEl.classList.toggle("unknown", fraction === null);
  if (fraction === null) {
    progressEl.removeAttribute("aria-valuenow");
    percentEl.textContent = "";
    return;
  }
  const percent = Math.min(100, Math.round(fraction * 100));
  progressEl.style.setProperty("--done", String(fraction));
  progressEl.setAttribute("aria-valuenow", String(percent));
  percentEl.textContent = `${percent}%`;
}

function show(status: UpdateStatus) {
  switch (status.state) {
    case "downloading": {
      const { version, downloaded, total } = status;
      titleEl.textContent = "Updating Achievement Shelf";
      versionEl.textContent = `v${version}`;
      versionEl.hidden = false;
      messageEl.textContent = "Downloading the new version";
      setDone(total ? downloaded / total : null);
      amountEl.textContent = total
        ? `${megabytes(downloaded)} of ${megabytes(total)} MB`
        : `${megabytes(downloaded)} MB`;
      break;
    }
    case "installing":
      messageEl.textContent = "Installing. The shelf will be back in a moment.";
      setDone(1);
      amountEl.textContent = "";
      break;
    case "failed":
      plateEl.classList.add("failed");
      messageEl.textContent = "The update didn't finish. It will try again next time the shelf starts.";
      progressEl.classList.remove("unknown");
      amountEl.textContent = "";
      percentEl.textContent = "";
      break;
  }
}

listen<UpdateStatus>("update-status", (event) => show(event.payload));
