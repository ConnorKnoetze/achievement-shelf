/** Rarity tiers by global unlock percentage, matching GOG Galaxy's bands. */
const TIERS: { min: number; name: string }[] = [
  { min: 30, name: "Common" },
  { min: 10, name: "Uncommon" },
  { min: 2.5, name: "Rare" },
  { min: 0.5, name: "Epic" },
  { min: 0, name: "Legendary" },
];

function findTier(percent: number) {
  return TIERS.find((t) => percent >= t.min) ?? TIERS[TIERS.length - 1];
}

/** The lowercase tier name such as "epic", or null when the rarity is unknown. */
export function rarityTier(percent: number | null): string | null {
  if (percent === null || !Number.isFinite(percent)) return null;
  return findTier(percent).name.toLowerCase();
}

/**
 * The printed grade label used in the setup windows: a tier block over or beside the
 * unlock percentage. Achievements without a known rarity are "Ungraded".
 */
export function gradeLabel(percent: number | null, className: string): HTMLSpanElement {
  const tier = rarityTier(percent);
  const label = document.createElement("span");
  label.className = className;
  const tierEl = document.createElement("span");
  tierEl.className = "grade-tier";
  tierEl.textContent = tier ?? "Ungraded";
  const pctEl = document.createElement("span");
  pctEl.className = "grade-pct";
  if (tier && percent !== null) {
    label.classList.add(`tier-${tier}`);
    pctEl.textContent = `${percent.toFixed(1)}%`;
    label.title = `${percent.toFixed(1)}% of players have this achievement`;
  } else {
    pctEl.textContent = "—";
    label.title = "No rarity data for this achievement";
  }
  label.append(tierEl, pctEl);
  return label;
}
