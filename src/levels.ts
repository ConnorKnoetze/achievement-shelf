/*
 * Where a carried trophy lands, shared by the shelf and the picker. Each trophy level holds its
 * own trophies, packed together and centred on its rail. The rail under the pointer picks the
 * level, if it has room, and the pointer's position along it picks the place among the others.
 */

/** A trophy's place: its level, top first, and its index among that level's movable trophies. */
export interface Place {
  level: number;
  index: number;
}

/** Measurements of the trophy levels, in the same coordinates as the pointer. */
export interface LevelGeometry {
  /** Where each level's band ends, top first; the last is Infinity, so every point has a level. */
  bandEnds: number[];
  /** The horizontal centre of each level's row of trophies. */
  centers: number[];
  /** Distance between the centres of neighbouring trophies. */
  pitch: number;
}

/** How much farther (px) past the halfway point the pointer goes before the place changes. */
const HYSTERESIS = 6;

/**
 * Where the carried trophy goes for a pointer at `x`, `y`. `others[l]` is how many trophies level
 * `l` holds besides it, and `movable[l]` how many of those it can go between (fixed ones follow
 * them). A full level other than its own keeps it where it is.
 */
export function dropPlace(
  x: number,
  y: number,
  geometry: LevelGeometry,
  others: number[],
  movable: number[],
  perRow: number,
  current: Place,
): Place {
  const level = geometry.bandEnds.findIndex((end) => y < end);
  if (level < 0 || (level !== current.level && others[level] >= perRow)) return current;
  const count = others[level] + 1;
  const center = geometry.centers[level];
  const centreOf = (i: number) => center + (i - (count - 1) / 2) * geometry.pitch;
  const index = Math.min(Math.max(Math.round((x - center) / geometry.pitch + (count - 1) / 2), 0), movable[level]);
  // Stays put until the pointer is clearly nearer another place on the same level.
  if (level === current.level && Math.abs(x - centreOf(current.index)) < geometry.pitch / 2 + HYSTERESIS) {
    return current;
  }
  return { level, index };
}

/** `levels` with `item` taken out of wherever it is and put at `place`. */
export function moveTo<T>(levels: T[][], item: T, place: Place): T[][] {
  const next = levels.map((level) => level.filter((other) => other !== item));
  next[place.level].splice(place.index, 0, item);
  return next;
}

/** Where `item` is in `levels`. */
export function findPlace<T>(levels: T[][], item: T): Place {
  const level = levels.findIndex((l) => l.includes(item));
  return { level, index: levels[level]?.indexOf(item) ?? -1 };
}
