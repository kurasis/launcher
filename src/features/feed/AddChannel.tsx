import { useEffect, useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { FeedProblem, MarkKind } from '../../lib/contracts';
import { feedProblem, ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import styles from './Feed.module.css';

const t = strings.feed;
const s = strings.feedSettings;
const PLACES: readonly MarkKind[] = ['youtube', 'twitch'];

/** The place an address plainly names, if it names one. */
function placeOf(input: string): MarkKind | null {
  const value = input.trim().toLowerCase();
  if (/(^|[/.])twitch\.tv(\/|$)/.test(value)) return 'twitch';
  if (/(^|[/.])(youtube\.com|youtu\.be)(\/|$)/.test(value) || value.startsWith('@')) {
    return 'youtube';
  }
  return null;
}

/** What a problem says for a channel of this place. */
function problemText(place: MarkKind, problem: FeedProblem): string {
  if (place === 'twitch' && problem === 'bad_input') return s.streamerBadInput;
  if (place === 'twitch' && problem === 'not_found') return s.streamerNotFound;
  return t.problems[problem];
}

/** Adds a channel and gives the name of the one added (or found already there). */
async function addTo(place: MarkKind, input: string): Promise<string | null> {
  if (place === 'youtube') {
    const before = new Set((await ipc.feedChannels()).map((channel) => channel.channelId));
    const after = await ipc.feedAddChannel(input);
    return (after.find((channel) => !before.has(channel.channelId)) ?? after.at(-1))?.title ?? null;
  }
  const before = new Set((await ipc.feedStreamers()).map((streamer) => streamer.login));
  const after = await ipc.feedAddStreamer(input);
  return (after.find((streamer) => !before.has(streamer.login)) ?? after.at(-1))?.name ?? null;
}

/**
 * The + at the end of the feed's chips: adds a YouTube or Twitch channel to
 * the feed right there, the same as Settings → Feed does.
 */
export function AddChannel({ onAdded }: { onAdded: (place: MarkKind, name: string) => void }) {
  const id = useId();
  const panelId = `${id}-panel`;
  const inputId = `${id}-input`;
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  const [place, setPlace] = useState<MarkKind>('youtube');
  const [input, setInput] = useState('');
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<FeedProblem | null>(null);

  const close = (refocus: boolean) => {
    setOpen(false);
    setProblem(null);
    if (refocus) buttonRef.current?.focus();
  };

  useEffect(() => {
    if (!open) return;
    inputRef.current?.focus();
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && rootRef.current?.contains(event.target)) return;
      setOpen(false);
      setProblem(null);
    };
    document.addEventListener('pointerdown', onPointerDown, true);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown, true);
    };
  }, [open]);

  return (
    <div
      ref={rootRef}
      className={styles.addRoot}
      onKeyDown={(event) => {
        if (event.key === 'Escape' && open) {
          event.preventDefault();
          close(true);
        }
      }}
    >
      <button
        ref={buttonRef}
        type="button"
        className={styles.addButton}
        aria-label={t.addChannel}
        title={t.addChannel}
        aria-expanded={open}
        aria-controls={open ? panelId : undefined}
        onClick={() => {
          if (open) close(false);
          else setOpen(true);
        }}
      >
        <span aria-hidden="true">+</span>
      </button>
      {open && (
        <div id={panelId} role="dialog" aria-label={t.addChannel} className={styles.addPanel}>
          <fieldset disabled={busy}>
            <legend>{t.addChannelPlace}</legend>
            {PLACES.map((item) => (
              <label key={item}>
                <input
                  type="radio"
                  name={`${id}-place`}
                  value={item}
                  checked={place === item}
                  onChange={() => {
                    setPlace(item);
                    setProblem(null);
                  }}
                />
                {t.places[item]}
              </label>
            ))}
          </fieldset>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              const value = input.trim();
              if (value === '') {
                setProblem('bad_input');
                return;
              }
              setBusy(true);
              setProblem(null);
              addTo(place, value)
                .then((name) => {
                  setInput('');
                  close(true);
                  onAdded(place, name ?? value);
                })
                .catch((error: unknown) => {
                  setProblem(feedProblem(error));
                })
                .finally(() => {
                  setBusy(false);
                });
            }}
          >
            <label htmlFor={inputId}>
              {place === 'youtube' ? s.channelInput : s.streamerInput}
            </label>
            <input
              ref={inputRef}
              id={inputId}
              className={forms.input}
              value={input}
              maxLength={200}
              disabled={busy}
              spellCheck={false}
              autoComplete="off"
              dir="auto"
              onChange={(event) => {
                setInput(event.target.value);
                const named = placeOf(event.target.value);
                if (named !== null) setPlace(named);
              }}
            />
            <Button type="submit" variant="primary" disabled={busy}>
              {busy ? s.adding : s.add}
            </Button>
          </form>
          {problem !== null && <Notice tone="danger">{problemText(place, problem)}</Notice>}
        </div>
      )}
    </div>
  );
}
