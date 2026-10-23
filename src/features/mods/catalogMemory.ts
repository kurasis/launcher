import type { ModsCollection, ModsKind, ModsSearchPage, ModsSort } from '../../lib/contracts';

/** The filters as the player is typing them. */
export interface Filters {
  text: string;
  game: string;
  loader: string;
  sort: ModsSort;
}

/** The search the list shows: the filters as last sent, with categories and a collection. */
export interface Committed extends Filters {
  categories: string[];
  collection: ModsCollection | null;
}

/** Where the catalog was: brought back when the player returns to it in this run. */
export interface Place {
  draft: Filters;
  committed: Committed;
  /** `null` when the search had not answered or had failed: it is asked again. */
  page: ModsSearchPage | null;
  scrollY: number;
}

/** Places kept in this run, the newest last. */
const places = new Map<string, Place>();
const MAX_PLACES = 12;

export function recall(key: string): Place | null {
  return places.get(key) ?? null;
}

export function remember(key: string, place: Place) {
  places.delete(key);
  places.set(key, place);
  while (places.size > MAX_PLACES) {
    const oldest = places.keys().next().value;
    if (oldest === undefined) break;
    places.delete(oldest);
  }
}

/** For tests: every mount starts afresh. */
export function forgetCatalog() {
  places.clear();
}

/** The search a mark belongs to; a mark of another search is not shown. */
export function searchKey(kind: ModsKind, committed: Committed): string {
  return JSON.stringify([
    kind,
    committed.text.trim(),
    committed.game.trim(),
    committed.loader,
    committed.sort,
    [...committed.categories].sort(),
    committed.collection?.id ?? null,
  ]);
}

/** How far down the list the player got in an earlier run of the launcher. */
export interface Mark {
  search: string;
  count: number;
  at: number;
}

const MARK_KEY = 'launcher.mods.place';
/** A mark older than this is forgotten. */
const MARK_DAYS = 30;
const DAY_MS = 86_400_000;
/** Marks from this run are not shown: the place itself comes back. */
const STARTED = Date.now();

/** The mark of an earlier run, if it is sound. */
export function readMark(): Mark | null {
  try {
    const raw = window.localStorage.getItem(MARK_KEY);
    if (raw === null || raw.length > 2048) return null;
    const value = JSON.parse(raw) as unknown;
    if (typeof value !== 'object' || value === null) return null;
    const { search, count, at } = value as Record<string, unknown>;
    if (
      typeof search !== 'string' ||
      search.length > 1024 ||
      typeof count !== 'number' ||
      !Number.isInteger(count) ||
      count < 1 ||
      count > 10_000 ||
      typeof at !== 'number' ||
      !Number.isFinite(at) ||
      at >= STARTED ||
      Date.now() - at > MARK_DAYS * DAY_MS
    ) {
      return null;
    }
    return { search, count, at };
  } catch {
    return null;
  }
}

export function writeMark(mark: Mark) {
  try {
    window.localStorage.setItem(MARK_KEY, JSON.stringify(mark));
  } catch {
    // Storage may be off or full: the mark is only a convenience.
  }
}
