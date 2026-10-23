import { type RefObject, useEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import type { WorldsPreview, WorldsProblem } from '../../lib/contracts';
import { ipc, worldsProblemOf } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { worldsErrorText } from '../worlds/useWorlds';
import { focusLost } from './focus';
import styles from './Friends.module.css';

const t = strings.friends;
/** A world's facts are read again after this long (who hosts it changes). */
const PREVIEW_HOLDS_MS = 5 * 60_000;
/** A code the service refused is asked again only after this long. */
const REFUSAL_HOLDS_MS = 60_000;
/**
 * Cards look by themselves at most this many times in this long; after that each one
 * waits for a press. The service counts the player's looks, and a chat full of codes
 * from someone else must not use them all up.
 */
const LOOKS_BY_THEMSELVES = 12;
const LOOKS_WINDOW_MS = 10 * 60_000;
/** What the service says of a code itself: no Join to offer. Others pass (the network). */
const REFUSALS: readonly WorldsProblem[] = [
  'invalid',
  'invite_invalid',
  'not_found',
  'forbidden',
  'banned',
];

type Looked =
  | { preview: WorldsPreview }
  /** The code itself was refused: expired, used up, not for this player. */
  | { refused: string }
  /** Asking did not work this time (the network, the service busy): ask again later. */
  | { failed: string };

/** What each code showed, kept while the launcher runs: a chat opened again asks nothing. */
const looked = new Map<string, { at: number; result: Looked }>();
const looking = new Map<string, Promise<Looked>>();
/** When cards looked by themselves, within the last window. */
const lookedByThemselves: number[] = [];

/** What is still known of a code, if it is recent enough to show. */
function known(code: string): Looked | null {
  const kept = looked.get(code);
  if (kept === undefined) return null;
  const holds = 'preview' in kept.result ? PREVIEW_HOLDS_MS : REFUSAL_HOLDS_MS;
  return Date.now() - kept.at < holds ? kept.result : null;
}

/** Whether a card may look without a press now; counts the look when it may. */
function mayLookByItself(): boolean {
  const now = Date.now();
  while (lookedByThemselves.length > 0 && now - (lookedByThemselves[0] ?? now) >= LOOKS_WINDOW_MS) {
    lookedByThemselves.shift();
  }
  if (lookedByThemselves.length >= LOOKS_BY_THEMSELVES) return false;
  lookedByThemselves.push(now);
  return true;
}

/** The facts of the world behind a code: read only, nothing is joined. */
function look(code: string): Promise<Looked> {
  const kept = known(code);
  if (kept !== null) return Promise.resolve(kept);
  const under = looking.get(code);
  if (under !== undefined) return under;
  const question = ipc
    .worldsPreview(code)
    .then(
      (preview): Looked => ({ preview }),
      (e: unknown): Looked => {
        const problem = worldsProblemOf(e);
        return problem !== null && REFUSALS.includes(problem)
          ? { refused: worldsErrorText(e) }
          : { failed: worldsErrorText(e) };
      },
    )
    .then((result) => {
      if (!('failed' in result)) looked.set(code, { at: Date.now(), result });
      return result;
    })
    .finally(() => {
      looking.delete(code);
    });
  looking.set(code, question);
  return question;
}

/** True once the element has come into view; at once where that cannot be told. */
function useSeen(element: RefObject<HTMLElement | null>): boolean {
  const [seen, setSeen] = useState(() => typeof IntersectionObserver === 'undefined');
  useEffect(() => {
    const node = element.current;
    if (seen || node === null) return;
    const observer = new IntersectionObserver((records) => {
      if (records.some((record) => record.isIntersecting)) setSeen(true);
    });
    observer.observe(node);
    return () => {
      observer.disconnect();
    };
  }, [element, seen]);
  return seen;
}

/**
 * An invite to a world in the chat. Once in view the card shows what the world is (its
 * name, version, mods and whether it is open now); the invited player joins the room
 * with a press and Worlds opens on it. The one who sent it can open the room. When the
 * card may not look by itself, or looking failed, a press on Look asks.
 */
export function WorldInviteCard({
  code,
  from,
  mine,
  onOpen,
}: {
  code: string;
  from: string;
  mine: boolean;
  /** Shows the room in Worlds. */
  onOpen: (room: string) => void;
}) {
  const card = useRef<HTMLDivElement>(null);
  const seen = useSeen(card);
  // What is known shows at once; nothing flickers on a chat opened again.
  const [result, setResult] = useState<Looked | null>(() => known(code));
  // No look to spare: the card waits for a press.
  const [waiting, setWaiting] = useState(false);
  const [asked, setAsked] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const live = useRef(true);
  // The focus is on one of the card's buttons, which may go when the answer comes.
  const hadFocus = useRef(false);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);

  // Once in view: what is known, else a look of its own while there are looks to spare.
  useEffect(() => {
    if (!seen) return;
    const spare = known(code) !== null || looking.has(code) || mayLookByItself();
    let on = true;
    void (spare ? look(code) : Promise.resolve(null)).then((next) => {
      if (!on) return;
      if (next === null) setWaiting(true);
      else setResult(next);
    });
    return () => {
      on = false;
    };
  }, [seen, code]);

  // Look went with the answer (the facts came, or the code was refused): the focus goes
  // to what the card offers now, or to the card itself.
  useEffect(() => {
    if (!hadFocus.current || !focusLost()) return;
    hadFocus.current = false;
    (card.current?.querySelector<HTMLElement>('button') ?? card.current)?.focus();
  }, [result]);

  const ask = () => {
    if (asked) return;
    setAsked(true);
    void look(code)
      .then((next) => {
        if (live.current) setResult(next);
      })
      .finally(() => {
        if (live.current) setAsked(false);
      });
  };

  const preview = result !== null && 'preview' in result ? result.preview : null;
  const refused = result !== null && 'refused' in result ? result.refused : null;
  const failed = result !== null && 'failed' in result ? result.failed : null;
  const join = () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    ipc
      .worldsRedeem(code)
      .then((room) => {
        onOpen(room.id);
      })
      .catch((e: unknown) => {
        setError(worldsErrorText(e));
      })
      .finally(() => {
        setBusy(false);
      });
  };

  return (
    <div
      ref={card}
      className={styles.card}
      // Takes the focus when the button that had it went with the answer.
      tabIndex={-1}
      onFocus={() => {
        hadFocus.current = true;
      }}
      onBlur={(event) => {
        const next = event.relatedTarget;
        if (next instanceof Node && event.currentTarget.contains(next)) return;
        // Gone elsewhere, or to nothing while the button stays (a press on text).
        const target = event.target;
        queueMicrotask(() => {
          if (next !== null || target.isConnected) hadFocus.current = false;
        });
      }}
    >
      <p className={styles.cardTitle}>
        {mine ? t.youInvitedWorld : t.invitedToWorld({ nick: from })}
      </p>
      {preview !== null && (
        <p className={styles.cardFacts}>
          <span dir="auto">
            {preview.mods === null
              ? t.worldFacts({ name: preview.name, version: preview.minecraft })
              : t.worldFactsMods({
                  name: preview.name,
                  version: preview.minecraft,
                  mods: preview.mods,
                })}
          </span>
          {preview.hosting && (
            <span className={styles.presence} data-online="true">
              <span className={styles.dot} aria-hidden="true" />
              <span className={styles.presenceText}>{t.worldOpen}</span>
            </span>
          )}
        </p>
      )}
      {(refused ?? failed) !== null && <p className={styles.muted}>{refused ?? failed}</p>}
      {/* A code the service refused (expired, used up) offers nothing to press. */}
      {refused === null && (
        <div className={styles.inline}>
          {mine && preview !== null && (
            <Button
              size="small"
              variant="ghost"
              onClick={() => {
                onOpen(preview.room);
              }}
            >
              {t.openRoom}
            </Button>
          )}
          {!mine && (
            <Button size="small" variant="primary" aria-disabled={busy || undefined} onClick={join}>
              {strings.worlds.code.accept}
            </Button>
          )}
          {preview === null && (waiting || failed !== null) && (
            <Button size="small" variant="ghost" aria-disabled={asked || undefined} onClick={ask}>
              {strings.worlds.code.look}
            </Button>
          )}
        </div>
      )}
      {error !== null && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
