/**
 * The feed's mosaic (D-072, concept A, "variant 2"): cards of different heights in
 * columns, laid out by CSS grid. The grid's rows are a few pixels each and every card
 * spans as many as its height needs, so the grid's own auto-placement puts each card,
 * in feed order, at the top of the shortest column. The DOM keeps the feed's order, so
 * reading, Tab and screen readers follow it.
 */

/** Rows a card of `height` spans, its gap below included: at least one. */
export function rowSpan(height: number, gap: number, row: number): number {
  if (!(row > 0) || !Number.isFinite(height) || height <= 0) return 1;
  return Math.max(1, Math.ceil((height + Math.max(0, gap)) / row));
}

/** The widest and the tallest a picture is drawn: 16:9 to 4:5. */
const WIDEST = 16 / 9;
const TALLEST = 4 / 5;

/** A picture's shape from its size, kept between 4:5 and 16:9; 16:9 when unknown. */
export function pictureAspect(width: number | null, height: number | null): number {
  if (width === null || height === null || !(width > 0) || !(height > 0)) return WIDEST;
  const aspect = width / height;
  if (!Number.isFinite(aspect)) return WIDEST;
  return Math.min(WIDEST, Math.max(TALLEST, aspect));
}

export const ASPECT = { wide: WIDEST, tall: TALLEST, short: 9 / 16 } as const;

/** How many colors letter avatars come in. */
export const AVATAR_TONES = 5;

/** FNV-1a over the name: the same channel keeps its color wherever it shows. */
export function avatarTone(name: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < name.length; i += 1) {
    hash ^= name.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0) % AVATAR_TONES;
}

/** The first letter of a name, as a letter avatar shows it; "?" for a blank name. */
export function initial(name: string): string {
  const trimmed = name.trim();
  if (trimmed === '') return '?';
  let first: string | undefined;
  if (typeof Intl.Segmenter === 'function') {
    for (const { segment } of new Intl.Segmenter(undefined, { granularity: 'grapheme' }).segment(
      trimmed,
    )) {
      first = segment;
      break;
    }
  }
  first ??= Array.from(trimmed)[0] ?? '?';
  return first.toLocaleUpperCase();
}
