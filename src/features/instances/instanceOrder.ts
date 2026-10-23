// Ordering and finding installations in the list (concept A, Installations).
// Pure functions: the component passes the language and the loader names in.
import type { InstanceSummary, LoaderKind } from '../../lib/contracts';

export type InstanceSort = 'recent' | 'name' | 'version';

export const INSTANCE_SORTS: readonly InstanceSort[] = ['recent', 'name', 'version'];

/** Search and sort appear only when the list is longer than this. */
export const FIND_THRESHOLD = 6;

/** The longest text the search field takes. */
export const QUERY_MAX_LENGTH = 64;

export interface OrderOptions {
  /** What the player typed; trimmed and cut to QUERY_MAX_LENGTH here. */
  query: string;
  sort: InstanceSort;
  /** The interface's locale for comparing names, e.g. `ru` or `ar-u-nu-latn`. */
  locale: string;
  /** The loader's name as the interface shows it ("Fabric"), so it can be found too. */
  loaderName: (kind: LoaderKind) => string;
}

/** Case, accents and Arabic marks do not matter when finding: "Ёлка" finds "елка". */
function fold(text: string): string {
  return text.normalize('NFKD').replace(/\p{M}/gu, '').toLowerCase();
}

/** The search as it is compared; empty when there is nothing to find. */
export function normalizeQuery(query: string): string {
  return fold(
    query
      .slice(0, QUERY_MAX_LENGTH * 2)
      .trim()
      .slice(0, QUERY_MAX_LENGTH),
  );
}

function matches(
  instance: InstanceSummary,
  folded: string,
  loaderName: OrderOptions['loaderName'],
) {
  const loader = instance.loader === null ? '' : loaderName(instance.loader.kind);
  return [instance.displayName, instance.versionId, loader].some((text) =>
    fold(text).includes(folded),
  );
}

/** A timestamp for ordering; missing or broken ones come last. */
function time(value: string | null): number {
  if (value === null) return Number.NEGATIVE_INFINITY;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? Number.NEGATIVE_INFINITY : parsed;
}

function collator(locale: string): Intl.Collator {
  const options: Intl.CollatorOptions = { numeric: true, sensitivity: 'base' };
  try {
    return new Intl.Collator(locale, options);
  } catch {
    // A broken locale tag must not empty the list.
    return new Intl.Collator(undefined, options);
  }
}

interface ParsedVersion {
  /** Newer kinds of version ids rank higher: releases and their snapshots, then weekly
   *  snapshots, beta, alpha, classic, infdev and pre-classic. */
  era: number;
  numbers: number[];
  /** What follows the numbers ("-snapshot-4", "-rc1"); empty for a release. */
  suffix: string;
}

const RELEASE = /^(\d{1,9}(?:\.\d{1,9}){0,7})(.*)$/s;
const WEEKLY = /^(\d{2})w(\d{2})(\w*)$/i;
const LEGACY: readonly (readonly [RegExp, number])[] = [
  [/^b(\d{1,9}(?:[._]\d{1,9}){0,7})(.*)$/is, 4],
  [/^a(\d{1,9}(?:[._]\d{1,9}){0,7})(.*)$/is, 3],
  [/^c(\d{1,9}(?:[._]\d{1,9}){0,7})(.*)$/is, 2],
  [/^inf-?(\d{1,9}(?:[._]\d{1,9}){0,7})(.*)$/is, 1],
  [/^rd-?(\d{1,9}(?:[._]\d{1,9}){0,7})(.*)$/is, 0],
];

function numbersOf(text: string): number[] {
  return text.split(/[._]/).map(Number);
}

/** Reads a game version id; null when it has no numbers to compare. */
export function parseVersion(versionId: string): ParsedVersion | null {
  const id = versionId.trim().slice(0, 128);
  const weekly = WEEKLY.exec(id);
  if (weekly !== null) {
    return { era: 5, numbers: [Number(weekly[1]), Number(weekly[2])], suffix: weekly[3] ?? '' };
  }
  const release = RELEASE.exec(id);
  if (release !== null) {
    return { era: 6, numbers: numbersOf(release[1] ?? ''), suffix: release[2] ?? '' };
  }
  for (const [pattern, era] of LEGACY) {
    const legacy = pattern.exec(id);
    if (legacy !== null) {
      return { era, numbers: numbersOf(legacy[1] ?? ''), suffix: legacy[2] ?? '' };
    }
  }
  return null;
}

/**
 * Newest first: by kind, then number by number (1.21.10 after 1.21.9), a release
 * before the snapshots and pre-releases of the same numbers, unreadable ids last.
 */
export function compareVersions(
  a: string,
  b: string,
  compareText: (x: string, y: string) => number,
) {
  const left = parseVersion(a);
  const right = parseVersion(b);
  if (left === null || right === null) {
    if (left !== null) return -1;
    if (right !== null) return 1;
    return compareText(a, b);
  }
  if (left.era !== right.era) return right.era - left.era;
  const length = Math.max(left.numbers.length, right.numbers.length);
  for (let i = 0; i < length; i += 1) {
    const difference = (right.numbers[i] ?? 0) - (left.numbers[i] ?? 0);
    if (difference !== 0) return difference;
  }
  if (left.suffix === right.suffix) return 0;
  if (left.suffix === '') return -1;
  if (right.suffix === '') return 1;
  // Later snapshots and pre-releases first: "-snapshot-4" before "-snapshot-3".
  return compareText(right.suffix, left.suffix);
}

/**
 * The installations to show, filtered by the search and in the chosen order. The
 * list given is never changed.
 */
export function orderInstances(
  instances: readonly InstanceSummary[],
  { query, sort, locale, loaderName }: OrderOptions,
): InstanceSummary[] {
  const folded = normalizeQuery(query);
  const found =
    folded === '' ? [...instances] : instances.filter((i) => matches(i, folded, loaderName));
  const text = collator(locale).compare;
  const byName = (a: InstanceSummary, b: InstanceSummary) =>
    text(a.displayName, b.displayName) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
  const compare: Record<InstanceSort, (a: InstanceSummary, b: InstanceSummary) => number> = {
    recent: (a, b) =>
      compareTimes(time(b.lastPlayedAt), time(a.lastPlayedAt)) ||
      compareTimes(time(b.createdAt), time(a.createdAt)) ||
      byName(a, b),
    name: (a, b) => byName(a, b),
    version: (a, b) => compareVersions(a.versionId, b.versionId, text) || byName(a, b),
  };
  return found.sort(compare[sort]);
}

/** Like a - b, but two missing times are equal instead of NaN. */
function compareTimes(a: number, b: number): number {
  if (a === b) return 0;
  return a < b ? -1 : 1;
}
