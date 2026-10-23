import { type Ref, useId } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import { Screen } from '../../components/Screen';
import { Segmented } from '../../components/Segmented';
import forms from '../../components/forms.module.css';
import {
  type ImportNote,
  type SkinUploads,
  type SkinsProblem,
  WARDROBE_NAME_CHARS,
} from '../../lib/contracts';
import { strings } from '../../lib/strings';
import { type Draft, durationsToSend, trialTexture, typedDurations } from './draft';
import { SkinPreview } from './SkinPreview';
import { type Outfit, SkinStage } from './SkinStage';
import styles from './Skins.module.css';
import { FRAME_MS_MAX, FRAME_MS_MIN, frameHeight, parseFrameMs } from './textures';
import { Toggle } from './Toggle';
import { firstChars, uploadsLine, wardrobeName } from './closet';

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

/** Seconds of one round of the frames, to a tenth. */
function cycleSeconds(durations: readonly (number | null)[]): number {
  return Math.round(durations.reduce<number>((sum, ms) => sum + (ms ?? 0), 0) / 100) / 10;
}

interface Props {
  draft: Draft;
  /** What the account wears, for the comparison. */
  worn: Outfit;
  nick: string | null;
  uploads: SkinUploads | null;
  /** The wardrobe has no room for another of this kind: it can only be worn. */
  full: boolean;
  /** A save is on its way: the actions wait. */
  busy: boolean;
  problem: SkinsProblem | null;
  headingRef: Ref<HTMLHeadingElement>;
  onChange: (next: Draft) => void;
  onCancel: () => void;
  onKeep: () => void;
  onKeepAndWear: () => void;
}

/**
 * A picked or taken skin or cape before it is kept (board A-SkinNew): the character
 * wearing it with its frames under it; what the check found; its name, arms and frame
 * durations; then keep it, keep and wear it, or not.
 */
export function NewSkin({
  draft,
  worn,
  nick,
  uploads,
  full,
  busy,
  problem,
  headingRef,
  onChange,
  onCancel,
  onKeep,
  onKeepAndWear,
}: Props) {
  const s = strings.accounts.skins;
  const t = s.picked;
  const n = s.newSkin;
  const { picked } = draft;
  const skin = picked.kind === 'skin';
  const checksId = useId();
  const framesHintId = useId();
  const nameBadId = useId();
  const typed = typedDurations(draft);
  const name = wardrobeName(draft.name);
  const ready = durationsToSend(draft) !== undefined && name !== null;
  const animated = picked.frames > 1;
  const height = frameHeight(picked);
  const clamped = picked.notes.filter(
    (note): note is Extract<ImportNote, { kind: 'clamped' }> => note.kind === 'clamped',
  );
  const others = picked.notes.filter((note) => note.kind !== 'clamped');
  const set = (patch: Partial<Draft>) => {
    onChange({ ...draft, ...patch });
  };
  const trial: Outfit = skin
    ? { skin: trialTexture(draft), cape: worn.cape }
    : { skin: worn.skin, cape: trialTexture(draft) };
  // The file's name keeps its own direction inside the line, which keeps the screen's.
  const shownName = `\u2068${picked.name}\u2069`;
  const source =
    picked.slim !== null
      ? n.byNick({ nick: shownName })
      : picked.source === 'png'
        ? n.plainFile({ name: shownName, width: picked.width, height: picked.height })
        : n.fromFile({ name: shownName, width: picked.width, height: picked.height });
  const armsHint =
    picked.slim !== null
      ? n.ownerArms
      : draft.slimGuess === null
        ? null
        : draft.slimGuess
          ? t.guessSlim
          : t.guessClassic;

  return (
    <Screen
      title={skin ? t.titleSkin : t.titleCape}
      headingRef={headingRef}
      back={
        <button
          type="button"
          className={styles.back}
          aria-disabled={busy || undefined}
          onClick={() => {
            if (!busy) onCancel();
          }}
        >
          <Icon name="back" size={18} className={styles.mirror} />
          {n.back}
        </button>
      }
      actions={<span className={styles.source}>{source}</span>}
    >
      <div className={styles.newColumns}>
        <SkinStage
          worn={worn}
          trial={trial}
          nick={nick}
          frame={draft.frame}
          below={<FrameStrip draft={draft} onChange={onChange} />}
        />
        <form
          className={styles.newForm}
          aria-label={skin ? t.titleSkin : t.titleCape}
          onSubmit={(event) => {
            event.preventDefault();
            if (ready && !busy) onKeepAndWear();
          }}
        >
          <section className={styles.box} aria-labelledby={checksId}>
            <h3 id={checksId} className={styles.boxTitle}>
              {n.checks}
            </h3>
            <ul className={styles.checks}>
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
          </section>

          <div className={styles.box}>
            <Field label={s.wardrobe.nameLabel} hint={s.wardrobe.nameHint}>
              {(props) => (
                <input
                  {...props}
                  aria-describedby={
                    name === null
                      ? `${props['aria-describedby'] ?? ''} ${nameBadId}`.trim()
                      : props['aria-describedby']
                  }
                  className={forms.input}
                  value={draft.name}
                  dir="auto"
                  autoComplete="off"
                  spellCheck={false}
                  aria-invalid={name === null}
                  onChange={(event) => {
                    // Cut by characters, so a cut never splits one; one over shows why.
                    set({ name: firstChars(event.target.value, WARDROBE_NAME_CHARS + 1) });
                  }}
                />
              )}
            </Field>
            {name === null && (
              <p id={nameBadId} className={styles.bad}>
                {s.problems.bad_name}
              </p>
            )}

            {skin && (
              <div className={styles.formRow}>
                <span className={styles.label}>{s.model}</span>
                <Segmented
                  label={s.model}
                  options={[
                    { value: false, label: s.classic },
                    { value: true, label: s.slim },
                  ]}
                  value={draft.slim}
                  pending={busy}
                  onChange={(slim) => {
                    set({ slim });
                  }}
                />
                {armsHint !== null && <p className={styles.hint}>{armsHint}</p>}
              </div>
            )}

            {animated && (
              <fieldset className={styles.framesBlock}>
                <legend className={styles.framesLegend}>
                  <span className={styles.label}>{t.frames}</span>
                  <span className={styles.hint}>
                    {t.cycle({ count: picked.frames, seconds: cycleSeconds(typed) })}
                  </span>
                </legend>
                <Toggle
                  label={t.same}
                  on={draft.same}
                  onChange={(same) => {
                    set({ same });
                  }}
                />
                {draft.same ? (
                  <label className={styles.frameMs}>
                    <span>{t.eachFrame}</span>
                    <input
                      className={forms.input}
                      type="number"
                      inputMode="numeric"
                      min={FRAME_MS_MIN}
                      max={FRAME_MS_MAX}
                      step={10}
                      dir="ltr"
                      aria-describedby={framesHintId}
                      aria-invalid={parseFrameMs(draft.sameText) === null}
                      value={draft.sameText}
                      onChange={(event) => {
                        set({ sameText: event.target.value.slice(0, 6) });
                      }}
                    />
                  </label>
                ) : (
                  <ol className={styles.frameGrid}>
                    {draft.texts.map((text, i) => (
                      <li key={i}>
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
                            aria-label={t.frameMs({ frame: i + 1 })}
                            aria-describedby={framesHintId}
                            aria-invalid={typed[i] === null}
                            value={text}
                            onChange={(event) => {
                              const texts = [...draft.texts];
                              texts[i] = event.target.value.slice(0, 6);
                              set({ texts });
                            }}
                          />
                        </label>
                      </li>
                    ))}
                  </ol>
                )}
                <p
                  id={framesHintId}
                  className={typed.some((ms) => ms === null) ? styles.bad : styles.hint}
                >
                  {n.framesHint}
                </p>
              </fieldset>
            )}
          </div>

          <Notice tone="info">
            {uploadsLine(uploads)} {n.serverCheck}
          </Notice>
          {full && <Notice tone="warning">{n.fullNote[picked.kind]}</Notice>}
          {problem !== null && <Notice tone="warning">{s.problems[problem]}</Notice>}

          <div className={styles.newActions}>
            <Button
              aria-disabled={busy || undefined}
              onClick={() => {
                if (!busy) onCancel();
              }}
            >
              {s.cancel}
            </Button>
            {!full && (
              <Button
                disabled={!ready}
                aria-disabled={busy || undefined}
                onClick={() => {
                  if (!busy) onKeep();
                }}
              >
                <Icon name="bookmark" size={18} />
                {n.keep}
              </Button>
            )}
            <Button
              type="submit"
              variant="primary"
              disabled={!ready}
              aria-disabled={busy || undefined}
            >
              <Icon name="check" size={18} />
              {full ? n.wearOnly : n.keepAndWear}
            </Button>
          </div>
        </form>
      </div>
    </Screen>
  );
}

/** The frames under the character: play or hold, step, and each frame to hold. */
function FrameStrip({ draft, onChange }: { draft: Draft; onChange: (next: Draft) => void }) {
  const s = strings.accounts.skins;
  const t = s.picked;
  const count = draft.picked.frames;
  if (count < 2) return null;
  const texture = trialTexture(draft);
  const typed = typedDurations(draft);
  const held = draft.frame;
  const step = (by: number) => {
    onChange({ ...draft, frame: ((held ?? 0) + by + count) % count });
  };
  return (
    <div className={styles.frameStrip} role="group" aria-label={t.frames}>
      <div className={styles.frameControls}>
        <button
          type="button"
          className={styles.chip}
          aria-label={t.previous}
          title={t.previous}
          onClick={() => {
            step(-1);
          }}
        >
          <Icon name="prev" size={16} className={styles.mirror} />
        </button>
        <button
          type="button"
          className={styles.chip}
          onClick={() => {
            onChange({ ...draft, frame: held === null ? 0 : null });
          }}
        >
          <Icon name={held === null ? 'pause' : 'play'} size={16} />
          {held === null ? t.pause : t.play}
        </button>
        <button
          type="button"
          className={styles.chip}
          aria-label={t.next}
          title={t.next}
          onClick={() => {
            step(1);
          }}
        >
          <Icon name="next" size={16} className={styles.mirror} />
        </button>
        <span className={styles.frameLabel} aria-live="polite">
          {held === null
            ? t.cycle({ count, seconds: cycleSeconds(typed) })
            : t.frameOf({ frame: held + 1, count, ms: typed[held] ?? 0 })}
        </span>
      </div>
      <ol className={styles.frameThumbs}>
        {typed.map((ms, i) => (
          <li key={i}>
            <button
              type="button"
              className={styles.frameThumb}
              aria-pressed={held === i}
              aria-label={t.showFrame({ frame: i + 1 })}
              onClick={() => {
                onChange({ ...draft, frame: held === i ? null : i });
              }}
            >
              <SkinPreview {...texture} frame={i} small label="" />
            </button>
            <span className={styles.frameThumbMs}>{ms === null ? '—' : s.newSkin.ms({ ms })}</span>
          </li>
        ))}
      </ol>
    </div>
  );
}
