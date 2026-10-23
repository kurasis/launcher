import { useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { useFocusBack } from '../../components/useFocusBack';
import forms from '../../components/forms.module.css';
import type { InstanceSummary, MemoryLimits } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './SettingsView.module.css';
import { localizeError } from '../../lib/strings';
import { recommendationText, useRecommendation } from './memoryLabel';
import { toInt } from '../../lib/format';

interface InstallationFormProps {
  instance: InstanceSummary;
  memory: MemoryLimits;
}

const t = strings.details;

/**
 * Memory, window size, fullscreen, Minecraft arguments and Launcher skins of
 * one installation.
 * Keyed by instance id by the parent.
 */
export function InstallationForm({ instance, memory }: InstallationFormProps) {
  const [heap, setHeap] = useState(String(instance.maxHeapMib));
  const [width, setWidth] = useState(String(instance.resolution.width));
  const [height, setHeight] = useState(String(instance.resolution.height));
  const [fullscreen, setFullscreen] = useState(instance.fullscreen);
  const [launcherSkins, setLauncherSkins] = useState(instance.launcherSkins);
  const skinsHintId = useId();
  const [gameArguments, setGameArguments] = useState(instance.gameArguments);
  const [saved, setSaved] = useState(false);
  // Every value sent is an argument: useCommand keeps a request ID only for
  // a retry with the same ones.
  const save = useCommand(
    (
      requestId: string,
      maxHeapMib: number,
      w: number,
      h: number,
      full: boolean,
      gameArgs: string,
      skins: boolean,
    ) =>
      ipc.updateInstanceSettings(
        {
          instanceId: instance.id,
          maxHeapMib,
          resolution: { width: w, height: h },
          fullscreen: full,
          gameArguments: gameArgs,
          launcherSkins: skins,
        },
        requestId,
      ),
  );
  const bounds = memory.resolution;
  const policy = memory.policy;
  const recommendation = useRecommendation({ instanceId: instance.id });
  // Use goes once the field holds the recommended amount: the focus goes to the field.
  const heapField = useRef<HTMLInputElement>(null);
  const recommended = recommendation !== null && String(recommendation.mib) === heap;
  useFocusBack(recommended, heapField);

  const submit = async () => {
    // Saving keeps the fields and Save as they are (a disabled control drops the focus),
    // but they do not change until it is done: Saved is said of what was sent.
    if (save.pending) return;
    setSaved(false);
    const result = await save.run(
      toInt(heap),
      toInt(width),
      toInt(height),
      fullscreen,
      gameArguments,
      launcherSkins,
    );
    if (result) setSaved(true);
  };

  return (
    <form
      className={forms.form}
      aria-label={t.title}
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
      {policy?.lowMemory === true && <Notice tone="warning">{t.lowMemory}</Notice>}
      {memory.error !== null && <Notice tone="danger">{localizeError(memory.error)}</Notice>}
      {instance.busy && <Notice tone="info">{t.busy}</Notice>}
      <Field
        label={t.memory}
        hint={
          policy
            ? t.memoryHint({
                min: policy.minHeapMib,
                max: policy.maxHeapMib,
              })
            : undefined
        }
      >
        {(props) => (
          <input
            {...props}
            className={[forms.input, styles.narrow].join(' ')}
            type="number"
            inputMode="numeric"
            min={policy?.minHeapMib}
            max={policy?.maxHeapMib}
            ref={heapField}
            step={256}
            value={heap}
            disabled={instance.busy}
            readOnly={save.pending}
            onChange={(event) => {
              setHeap(event.target.value);
              setSaved(false);
            }}
          />
        )}
      </Field>
      {recommendation !== null && (
        <div className={styles.actions}>
          <span className={styles.hint}>{recommendationText(recommendation)}</span>
          {!recommended && (
            <Button
              size="small"
              disabled={instance.busy}
              aria-disabled={save.pending || undefined}
              onClick={() => {
                if (save.pending) return;
                setHeap(String(recommendation.mib));
                setSaved(false);
              }}
            >
              {strings.memory.use}
            </Button>
          )}
        </div>
      )}
      <div className={styles.row}>
        <Field label={t.width}>
          {(props) => (
            <input
              {...props}
              className={[forms.input, styles.narrow].join(' ')}
              type="number"
              inputMode="numeric"
              min={bounds.minWidth}
              max={bounds.maxWidth}
              value={width}
              disabled={instance.busy}
              readOnly={save.pending}
              onChange={(event) => {
                setWidth(event.target.value);
                setSaved(false);
              }}
            />
          )}
        </Field>
        <Field label={t.height}>
          {(props) => (
            <input
              {...props}
              className={[forms.input, styles.narrow].join(' ')}
              type="number"
              inputMode="numeric"
              min={bounds.minHeight}
              max={bounds.maxHeight}
              value={height}
              disabled={instance.busy}
              readOnly={save.pending}
              onChange={(event) => {
                setHeight(event.target.value);
                setSaved(false);
              }}
            />
          )}
        </Field>
      </div>
      <p className={styles.hint}>
        {t.resolutionHint({
          minWidth: bounds.minWidth,
          maxWidth: bounds.maxWidth,
          minHeight: bounds.minHeight,
          maxHeight: bounds.maxHeight,
        })}
      </p>
      <label className={forms.choice}>
        <input
          type="checkbox"
          checked={fullscreen}
          disabled={instance.busy}
          onChange={(event) => {
            if (save.pending) return;
            setFullscreen(event.target.checked);
            setSaved(false);
          }}
        />
        {t.fullscreen}
      </label>
      <label className={forms.choice}>
        <input
          type="checkbox"
          checked={launcherSkins}
          disabled={instance.busy}
          aria-describedby={skinsHintId}
          onChange={(event) => {
            if (save.pending) return;
            setLauncherSkins(event.target.checked);
            setSaved(false);
          }}
        />
        {t.launcherSkins}
      </label>
      <p className={styles.hint} id={skinsHintId}>
        {t.launcherSkinsHint}
      </p>
      <Field label={t.gameArguments} hint={t.gameArgumentsHint}>
        {(props) => (
          <input
            {...props}
            className={forms.input}
            dir="ltr"
            spellCheck={false}
            value={gameArguments}
            disabled={instance.busy}
            readOnly={save.pending}
            onChange={(event) => {
              setGameArguments(event.target.value);
              setSaved(false);
            }}
          />
        )}
      </Field>
      {save.error !== null && <Notice tone="danger">{save.error}</Notice>}
      <div className={styles.actions}>
        <Button
          variant="primary"
          size="small"
          type="submit"
          disabled={instance.busy}
          aria-disabled={save.pending || undefined}
        >
          {t.save}
        </Button>
        {saved && (
          <span role="status" className={styles.saved}>
            {t.saved}
          </span>
        )}
      </div>
    </form>
  );
}
