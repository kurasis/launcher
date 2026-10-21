import { useId } from 'react';
import { Button } from '../../components/Button';
import forms from '../../components/forms.module.css';
import type { ImportNote } from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { SkinPreview } from './SkinPreview';
import styles from './Skins.module.css';
import { type Draft, durationsToSend, trialTexture, typedDurations } from './draft';
import { FRAME_MS_MAX, FRAME_MS_MIN, frameHeight, parseFrameMs } from './textures';

/** Largest file per kind, as the service takes it. */
const MAX_BYTES = { skin: 2 * 1024 * 1024, cape: 1024 * 1024 } as const;
/** Notes about durations shown one by one; the rest are counted. */
const SHOWN_CLAMPED = 3;

function noteText(note: ImportNote): string {
  const n = strings.accounts.skins.notes;
  switch (note.kind) {
    case 'merged':
      return n.merged({ from: note.from, to: note.to });
    case 'everyNth':
      return n.everyNth({ from: note.from, to: note.to, step: note.step });
    case 'clamped':
      return n.clamped({ frame: note.frame + 1, was: note.was, now: note.now });
  }
}

interface Props {
  draft: Draft;
  busy: boolean;
  onChange: (next: Draft) => void;
  onWear: () => void;
  onCancel: () => void;
}

/** A picked skin or cape: what the check found, the arms, the frames and how long
 * each shows, then wear it or not. */
export function NewTexture({ draft, busy, onChange, onWear, onCancel }: Props) {
  const s = strings.accounts.skins;
  const t = s.picked;
  const { picked } = draft;
  const skin = picked.kind === 'skin';
  const texture = trialTexture(draft);
  const sameId = useId();
  const hintId = useId();
  const typed = typedDurations(draft);
  const toSend = durationsToSend(draft);
  const valid = toSend !== undefined;
  const animated = picked.frames > 1;
  const height = frameHeight(picked);
  const cycle = typed.reduce<number>((sum, ms) => sum + (ms ?? 0), 0);
  const clamped = picked.notes.filter(
    (note): note is Extract<ImportNote, { kind: 'clamped' }> => note.kind === 'clamped',
  );
  const others = picked.notes.filter((note) => note.kind !== 'clamped');
  const set = (patch: Partial<Draft>) => {
    onChange({ ...draft, ...patch });
  };

  return (
    <form
      className={styles.newTexture}
      aria-label={skin ? t.titleSkin : t.titleCape}
      onSubmit={(event) => {
        event.preventDefault();
        if (valid && !busy) onWear();
      }}
    >
      <div className={styles.newHead}>
        <h4 className={styles.subtitle}>{skin ? t.titleSkin : t.titleCape}</h4>
        <span className={styles.hint} dir="auto">
          {picked.name}
        </span>
      </div>

      <ul className={styles.checks} aria-label={t.checks}>
        <li data-mark="ok">
          {picked.width > 64
            ? t.sizeHd({ width: picked.width, height, scale: picked.width / 64 })
            : t.size({ width: picked.width, height })}
        </li>
        <li data-mark="ok">{animated ? t.strip({ count: picked.frames }) : t.oneFrame}</li>
        <li data-mark="ok">
          {t.weight({
            kb: Math.max(1, Math.round(picked.bytes / 1024)),
            max: MAX_BYTES[picked.kind] / 1024,
          })}
        </li>
        {picked.source !== 'png' && (
          <li data-mark="info">{picked.source === 'gif' ? t.fromGif : t.fromApng}</li>
        )}
        {others.map((note) => (
          <li key={note.kind} data-mark="info">
            {noteText(note)}
          </li>
        ))}
        {clamped.slice(0, SHOWN_CLAMPED).map((note) => (
          <li key={`clamped-${String(note.frame)}`} data-mark="warn">
            {noteText(note)}
          </li>
        ))}
        {clamped.length > SHOWN_CLAMPED && (
          <li data-mark="warn">{t.moreClamped({ count: clamped.length - SHOWN_CLAMPED })}</li>
        )}
      </ul>

      {skin && (
        <fieldset className={forms.choices} disabled={busy}>
          <legend>{s.model}</legend>
          <label className={forms.choice}>
            <input
              type="radio"
              name="skin-model"
              checked={!draft.slim}
              onChange={() => {
                set({ slim: false });
              }}
            />
            {s.classic}
          </label>
          <label className={forms.choice}>
            <input
              type="radio"
              name="skin-model"
              checked={draft.slim}
              onChange={() => {
                set({ slim: true });
              }}
            />
            {s.slim}
          </label>
          {draft.slimGuess !== null && (
            <p className={styles.hint}>{draft.slimGuess ? t.guessSlim : t.guessClassic}</p>
          )}
        </fieldset>
      )}

      {animated && (
        <section className={styles.framesBlock} aria-label={t.frames}>
          <div className={styles.framesHead}>
            <h5 className={styles.subtitle}>{t.frames}</h5>
            <span className={styles.hint}>
              {t.cycle({ count: picked.frames, seconds: Math.round(cycle / 100) / 10 })}
            </span>
            <label className={styles.sameToggle}>
              <input
                type="checkbox"
                checked={draft.same}
                disabled={busy}
                onChange={(event) => {
                  set({ same: event.target.checked });
                }}
              />
              {t.same}
            </label>
          </div>
          {draft.same && (
            <div className={styles.sameMs}>
              <label htmlFor={sameId}>{t.eachFrame}</label>
              <input
                id={sameId}
                aria-describedby={hintId}
                className={forms.input}
                type="number"
                inputMode="numeric"
                min={FRAME_MS_MIN}
                max={FRAME_MS_MAX}
                step={10}
                dir="ltr"
                disabled={busy}
                aria-invalid={parseFrameMs(draft.sameText) === null}
                value={draft.sameText}
                onChange={(event) => {
                  set({ sameText: event.target.value.slice(0, 6) });
                }}
              />
              <span id={hintId} className={styles.hint}>
                {s.frameMsHint}
              </span>
            </div>
          )}
          <ol className={styles.frameGrid}>
            {draft.texts.map((text, i) => {
              const bad = !draft.same && typed[i] === null;
              return (
                <li key={i} className={styles.frameCell} data-held={draft.frame === i}>
                  <button
                    type="button"
                    className={styles.frameThumb}
                    aria-pressed={draft.frame === i}
                    aria-label={t.showFrame({ frame: i + 1 })}
                    onClick={() => {
                      set({ frame: draft.frame === i ? null : i });
                    }}
                  >
                    <SkinPreview {...texture} frame={i} small label="" />
                  </button>
                  <label className={styles.frameMs}>
                    <span>{t.frameNumber({ frame: i + 1 })}</span>
                    <input
                      className={forms.input}
                      type="number"
                      inputMode="numeric"
                      min={FRAME_MS_MIN}
                      max={FRAME_MS_MAX}
                      step={10}
                      dir="ltr"
                      disabled={busy || draft.same}
                      aria-label={t.frameMs({ frame: i + 1 })}
                      aria-invalid={bad}
                      value={draft.same ? draft.sameText : text}
                      onChange={(event) => {
                        const texts = [...draft.texts];
                        texts[i] = event.target.value.slice(0, 6);
                        set({ texts });
                      }}
                    />
                  </label>
                  {bad && <span className={styles.bad}>{s.frameMsHint}</span>}
                </li>
              );
            })}
          </ol>
        </section>
      )}

      <p className={styles.hint}>{s.limit}</p>
      <div className={forms.actions}>
        <Button type="submit" variant="primary" disabled={busy || !valid}>
          {t.wear}
        </Button>
        <Button disabled={busy} onClick={onCancel}>
          {s.cancel}
        </Button>
      </div>
    </form>
  );
}

/** Play, pause and step through the frames, over the character. */
export function FrameControls({
  draft,
  onChange,
}: {
  draft: Draft;
  onChange: (next: Draft) => void;
}) {
  const t = strings.accounts.skins.picked;
  const count = draft.picked.frames;
  if (count < 2) return null;
  const held = draft.frame;
  const step = (by: number) => {
    const from = held ?? 0;
    onChange({ ...draft, frame: (from + by + count) % count });
  };
  const ms = held === null ? null : typedDurations(draft)[held];
  return (
    <div className={styles.frameControls}>
      <button
        type="button"
        className={styles.chip}
        onClick={() => {
          onChange({ ...draft, frame: held === null ? 0 : null });
        }}
      >
        {held === null ? t.pause : t.play}
      </button>
      <button
        type="button"
        className={styles.chip}
        aria-label={t.previous}
        onClick={() => {
          step(-1);
        }}
      >
        ‹
      </button>
      <button
        type="button"
        className={styles.chip}
        aria-label={t.next}
        onClick={() => {
          step(1);
        }}
      >
        ›
      </button>
      {held !== null && (
        <span className={styles.frameLabel}>
          {t.frameOf({ frame: held + 1, count, ms: ms ?? 0 })}
        </span>
      )}
    </div>
  );
}
