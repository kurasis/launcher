import { useEffect, useState } from 'react';
import { ipc } from '../../lib/ipc';
import { isPngDataUrl } from './textures';

/** The core answers at most this many nicks at once (`MAX_HEAD_NICKS` in skins.rs). */
export const HEADS_AT_ONCE = 200;
/** A head found is asked for again after this long: a friend may change their skin. */
export const FOUND_HOLDS_MS = 10 * 60_000;
/** A nick without a head (no skin, or the service could not be reached) is asked again
 * after this long, as soon as the window comes back or a list shows it. */
export const MISSING_HOLDS_MS = 60_000;
/** Nicks remembered at most; the oldest go first. */
const KEPT = 2048;
/** A nick the skins service knows: the rest get the default face without asking. */
const NICK = /^[A-Za-z0-9_]{3,16}$/;

/** What the launcher knows of a nick's head, and since when. */
interface Known {
  head: string | null;
  at: number;
}

/** Heads by nick, shared by every list that shows players. */
const known = new Map<string, Known>();
/** Questions under way, by nick: a nick is asked once however many lists want it. */
const asking = new Map<string, Promise<void>>();

function stale(nick: string, now: number): boolean {
  const entry = known.get(nick);
  if (entry === undefined) return true;
  return now - entry.at >= (entry.head === null ? MISSING_HOLDS_MS : FOUND_HOLDS_MS);
}

function remember(nick: string, head: string | null) {
  // Set again at the end, so the oldest answers are the first to go.
  known.delete(nick);
  known.set(nick, { head, at: Date.now() });
  for (const oldest of known.keys()) {
    if (known.size <= KEPT) break;
    known.delete(oldest);
  }
}

/** Asks the core for these nicks' heads, up to 200 at a time; settles when all are known. */
function ask(nicks: readonly string[]): Promise<void> {
  const fresh = nicks.filter((nick) => !asking.has(nick));
  for (let start = 0; start < fresh.length; start += HEADS_AT_ONCE) {
    const batch = fresh.slice(start, start + HEADS_AT_ONCE);
    const question = ipc
      .skinsHeads(batch)
      .then((answer: unknown) => {
        const map =
          typeof answer === 'object' && answer !== null && !Array.isArray(answer)
            ? (answer as Record<string, unknown>)
            : {};
        for (const nick of batch) {
          const head: unknown = Object.prototype.hasOwnProperty.call(map, nick) ? map[nick] : null;
          remember(nick, isPngDataUrl(head) ? head : null);
        }
      })
      // Not reached: the default faces stay until the next try.
      .catch(() => {
        for (const nick of batch) remember(nick, known.get(nick)?.head ?? null);
      })
      .finally(() => {
        for (const nick of batch) {
          if (asking.get(nick) === question) asking.delete(nick);
        }
      });
    for (const nick of batch) asking.set(nick, question);
  }
  const questions = nicks.flatMap((nick) => {
    const question = asking.get(nick);
    return question === undefined ? [] : [question];
  });
  return Promise.all(questions).then(() => undefined);
}

function headsOf(key: string): ReadonlyMap<string, string> {
  const heads = new Map<string, string>();
  if (key === '') return heads;
  for (const nick of key.split(' ')) {
    const head = known.get(nick)?.head ?? null;
    if (head !== null) heads.set(nick, head);
  }
  return heads;
}

/**
 * The skin heads of players by nick (friends, members of a room), cut by the core from
 * the skins the service serves: one shared question per nick, kept between lists, asked
 * again when the window comes back after a while. A nick without a head, an answer that
 * is not a PNG, or a nick the service cannot know gets the default face.
 */
export function usePlayerHeads(nicks: readonly string[]): ReadonlyMap<string, string> {
  const key = [...new Set(nicks.filter((nick) => NICK.test(nick)))].sort().join(' ');
  const [shown, setShown] = useState(() => ({ for: key, heads: headsOf(key) }));
  const [tries, setTries] = useState(0);

  useEffect(() => {
    if (key === '') return;
    let live = true;
    const now = Date.now();
    const wanted = key.split(' ').filter((nick) => stale(nick, now));
    void (wanted.length === 0 ? Promise.resolve() : ask(wanted)).then(() => {
      if (live) setShown({ for: key, heads: headsOf(key) });
    });
    return () => {
      live = false;
    };
  }, [key, tries]);

  useEffect(() => {
    if (key === '') return;
    const onFocus = () => {
      setTries((n) => n + 1);
    };
    window.addEventListener('focus', onFocus);
    return () => {
      window.removeEventListener('focus', onFocus);
    };
  }, [key]);

  return shown.for === key ? shown.heads : headsOf(key);
}
