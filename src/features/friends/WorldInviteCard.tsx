import { type RefObject, useEffect, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import type { WorldsPreview } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { worldsErrorText } from '../worlds/useWorlds';
import styles from './Friends.module.css';

const t = strings.friends;
/** A world's facts are read again after this long (who hosts it changes). */
const PREVIEW_HOLDS_MS = 5 * 60_000;
/** A code the service refused is asked again only after this long. */
const REFUSAL_HOLDS_MS = 60_000;

type Looked = { preview: WorldsPreview } | { error: string };

/** What each code showed, kept while the launcher runs: a chat opened again asks nothing. */
const looked = new Map<string, { at: number; result: Looked }>();
const looking = new Map<string, Promise<Looked>>();

/** The facts of the world behind a code: read only, nothing is joined. */
function look(code: string): Promise<Looked> {
  const kept = looked.get(code);
  if (kept !== undefined) {
    const holds = 'preview' in kept.result ? PREVIEW_HOLDS_MS : REFUSAL_HOLDS_MS;
    if (Date.now() - kept.at < holds) return Promise.resolve(kept.result);
  }
  const under = looking.get(code);
  if (under !== undefined) return under;
  const question = ipc
    .worldsPreview(code)
    .then(
      (preview): Looked => ({ preview }),
      (e: unknown): Looked => ({ error: worldsErrorText(e) }),
    )
    .then((result) => {
      looked.set(code, { at: Date.now(), result });
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
 * with a press and Worlds opens on it. The one who sent it can open the room.
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
  const [result, setResult] = useState<Looked | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!seen) return;
    let live = true;
    void look(code).then((next) => {
      if (live) setResult(next);
    });
    return () => {
      live = false;
    };
  }, [seen, code]);

  const preview = result !== null && 'preview' in result ? result.preview : null;
  const refused = result !== null && 'error' in result ? result.error : null;
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
    <div ref={card} className={styles.card}>
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
      {refused !== null && <p className={styles.muted}>{refused}</p>}
      {mine && preview !== null && (
        <div className={styles.inline}>
          <Button
            size="small"
            variant="ghost"
            onClick={() => {
              onOpen(preview.room);
            }}
          >
            {t.openRoom}
          </Button>
        </div>
      )}
      {/* A code the service refused (expired, used up) offers nothing to press. */}
      {!mine && refused === null && (
        <div className={styles.inline}>
          <Button size="small" variant="primary" aria-disabled={busy || undefined} onClick={join}>
            {strings.worlds.code.accept}
          </Button>
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
