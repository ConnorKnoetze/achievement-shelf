/** Rarity tiers by global unlock percentage, matching GOG Galaxy's bands. */
const TIERS: { min: number; name: string }[] = [
  { min: 30, name: "Common" },
  { min: 10, name: "Uncommon" },
  { min: 2.5, name: "Rare" },
  { min: 0.5, name: "Epic" },
  { min: 0, name: "Legendary" },
];

/** A span such as "Epic · 1.8%", coloured by tier, or null when the rarity is unknown. */
export function rarityLabel(percent: number | null): HTMLSpanElement | null {
  if (percent === null || !Number.isFinite(percent)) return null;
  const tier = TIERS.find((t) => percent >= t.min) ?? TIERS[TIERS.length - 1];
  const span = document.createElement("span");
  span.className = `rarity rarity-${tier.name.toLowerCase()}`;
  span.textContent = `${tier.name} · ${percent.toFixed(1)}%`;
  span.title = `${percent.toFixed(1)}% of players have this achievement`;
  return span;
}
