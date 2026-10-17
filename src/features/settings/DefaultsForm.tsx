import { useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { MemoryLimits, SettingsView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';

const t = strings.settings;
const d = strings.details;

function toInt(text: string): number {
  return /^\d+$/.test(text.trim()) ? Number(text.trim()) : Number.NaN;
}

/** Memory and window size used for new installations. Rust validates the values. */
export function DefaultsForm({
  settings,
  memory,
}: {
  settings: SettingsView;
  memory: MemoryLimits;
}) {
  const policy = memory.policy;
  const bounds = memory.resolution;
  const [heap, setHeap] = useState(
    String(settings.defaultMaxHeapMib ?? policy?.recommendedHeapMib ?? ''),
  );
  const [width, setWidth] = useState(String(settings.defaultResolution.width));
  const [height, setHeight] = useState(String(settings.defaultResolution.height));
  const [saved, setSaved] = useState(false);
  const save = useCommand((requestId: string, mib: number, w: number, h: number) =>
    ipc.setAppSettings(
      { defaultMaxHeapMib: mib, defaultResolution: { width: w, height: h } },
      requestId,
    ),
  );
  const disabled = save.pending || !settings.writable;

  return (
    <form
      className={forms.form}
      aria-label={t.defaultsTitle}
      onSubmit={(event) => {
        event.preventDefault();
        setSaved(false);
        void save.run(toInt(heap), toInt(width), toInt(height)).then((r) => {
          if (r) setSaved(true);
        });
      }}
    >
      <Field
        label={d.memory}
        hint={
          policy
            ? d.memoryHint(policy.minHeapMib, policy.maxHeapMib, policy.recommendedHeapMib)
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
            disabled={disabled}
            onChange={(event) => {
              setHeap(event.target.value);
              setSaved(false);
            }}
          />
        )}
      </Field>
      <div className={forms.row}>
        <Field label={d.width}>
          {(props) => (
            <input
              {...props}
              className={forms.input}
              type="number"
              inputMode="numeric"
              min={bounds.minWidth}
              max={bounds.maxWidth}
              value={width}
              disabled={disabled}
              onChange={(event) => {
                setWidth(event.target.value);
                setSaved(false);
              }}
            />
          )}
        </Field>
        <Field label={d.height}>
          {(props) => (
            <input
              {...props}
              className={forms.input}
              type="number"
              inputMode="numeric"
              min={bounds.minHeight}
              max={bounds.maxHeight}
              value={height}
              disabled={disabled}
              onChange={(event) => {
                setHeight(event.target.value);
                setSaved(false);
              }}
            />
          )}
        </Field>
      </div>
      {save.error !== null && <Notice tone="danger">{save.error}</Notice>}
      <div className={forms.actions}>
        <Button type="submit" disabled={disabled}>
          {t.defaultsSave}
        </Button>
        {saved && <span role="status">{t.defaultsSaved}</span>}
      </div>
    </form>
  );
}
