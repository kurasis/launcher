import type {
  FavoriteFeed,
  FeedItem,
  FeedMarks,
  FeedProblem,
  FeedStream,
  FeedVideo,
  FeedView,
  ServerAd,
} from '../../lib/contracts';
import type { FeedFilter } from './list';

/**
 * The feed screen as the player left it, kept while the launcher runs (never on
 * disk): going back to it shows the same cards at once, scrolled as they were, and
 * only what is new comes in.
 */
export interface FeedPlace {
  result: { view: FeedView | null; problem: FeedProblem | null } | null;
  own: FeedVideo[];
  ownStreams: FeedStream[];
  ads: ServerAd[];
  marks: FeedMarks;
  favorites: FavoriteFeed;
  watched: ReadonlySet<string>;
  filter: FeedFilter;
  scrollY: number;
}

/** One chip's list: the pages fetched so far. */
export interface ListPlace {
  pages: FeedItem[];
  next: string | null;
  /** The pages had failed: the feed the core keeps was shown instead. */
  kept: boolean;
  /** When its first page was last asked for (`Date.now()`). */
  checked: number;
  /** The order server ads take in it. */
  seed: number;
}

let place: FeedPlace | null = null;
/** Lists kept in this run, the newest last. */
const lists = new Map<string, ListPlace>();
const MAX_LISTS = 8;

export function recallFeed(): FeedPlace | null {
  return place;
}

export function rememberFeed(next: FeedPlace) {
  place = next;
}

export function recallList(key: string): ListPlace | null {
  return lists.get(key) ?? null;
}

export function rememberList(key: string, list: ListPlace) {
  lists.delete(key);
  lists.set(key, list);
  while (lists.size > MAX_LISTS) {
    const oldest = lists.keys().next().value;
    if (oldest === undefined) break;
    lists.delete(oldest);
  }
}

/** For tests: every mount starts afresh. */
export function forgetFeed() {
  place = null;
  lists.clear();
}
