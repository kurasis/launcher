import { useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { InstanceSummary, MemoryLimits } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './SettingsView.module.css';
import { localizeError } from '../../lib/i18n/errors';

interface InstallationFormProps {
  instance: InstanceSummary;
  memory: MemoryLimits;
}

const t = strings.details;

function toInt(text: string): number {
  return /^\d+$/.test(text.trim()) ? Number(text.trim()) : Number.NaN;
}

/** Memory and window size of one installation. Keyed by instance id by the parent. */
export function InstallationForm({ instance, memory }: InstallationFormProps) {
  const [heap, setHeap] = useState(String(instance.maxHeapMib));
  const [width, setWidth] = useState(String(instance.resolution.width));
  const [height, setHeight] = useState(String(instance.resolution.height));
  const [saved, setSaved] = useState(false);
  const save = useCommand((requestId: string, maxHeapMib: number, w: number, h: number) =>
    ipc.updateInstanceSettings(
      { instanceId: instance.id, maxHeapMib, resolution: { width: w, height: h } },
      requestId,
    ),
  );
  const bounds = memory.resolution;
  const policy = memory.policy;

  const submit = async () => {
    setSaved(false);
    const result = await save.run(toInt(heap), toInt(width), toInt(height));
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
            ? t.memoryHint(policy.minHeapMib, policy.maxHeapMib, policy.recommendedHeapMib)
            : undefined
        }
      >
        {(props) => (
          <input
            {...props}
            className={forms.input}
            type="number"
            inputMode="numeric"
            min={policy?.minHeapMib}
            max={policy?.maxHeapMib}
            step={256}
            value={heap}
            disabled={instance.busy}
            onChange={(event) => {
              setHeap(event.target.value);
              setSaved(false);
            }}
          />
        )}
      </Field>
      <div className={forms.row}>
        <Field label={t.width}>
          {(props) => (
            <input
              {...props}
              className={forms.input}
              type="number"
              inputMode="numeric"
              min={bounds.minWidth}
              max={bounds.maxWidth}
              value={width}
              disabled={instance.busy}
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
              className={forms.input}
              type="number"
              inputMode="numeric"
              min={bounds.minHeight}
              max={bounds.maxHeight}
              value={height}
              disabled={instance.busy}
              onChange={(event) => {
                setHeight(event.target.value);
                setSaved(false);
              }}
            />
          )}
        </Field>
      </div>
      <p className={styles.hint}>
        {t.resolutionHint(bounds.minWidth, bounds.maxWidth, bounds.minHeight, bounds.maxHeight)}
      </p>
      {save.error !== null && <Notice tone="danger">{save.error}</Notice>}
      <div className={forms.actions}>
        <Button variant="primary" type="submit" disabled={save.pending || instance.busy}>
          {t.save}
        </Button>
        {saved && <span role="status">{t.saved}</span>}
      </div>
    </form>
  );
}
