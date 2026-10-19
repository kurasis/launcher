import { useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { MemoryLimits, SettingsView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './SettingsView.module.css';
import { toInt } from '../../lib/format';

const t = strings.settings;
const d = strings.details;

/** Memory, window size and fullscreen used for new installations. Rust validates the values. */
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
  const [fullscreen, setFullscreen] = useState(settings.defaultFullscreen);
  // Recommended per kind of installation (D-058), or one fixed amount.
  const [fixed, setFixed] = useState(settings.defaultMaxHeapMib !== null);
  const [saved, setSaved] = useState(false);
  // Every value sent is an argument: useCommand keeps a request ID only for
  // a retry with the same ones.
  const save = useCommand((requestId: string, mib: number, w: number, h: number, full: boolean) =>
    ipc.setAppSettings(
      {
        defaultMaxHeapMib: mib,
        defaultResolution: { width: w, height: h },
        defaultFullscreen: full,
      },
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
        void save
          .run(fixed ? toInt(heap) : 0, toInt(width), toInt(height), fullscreen)
          .then((r) => {
            if (r) setSaved(true);
          });
      }}
    >
      <fieldset className={forms.choices} disabled={disabled}>
        <legend>{d.memory}</legend>
        <label className={forms.choice}>
          <input
            type="radio"
            name="default-memory"
            checked={!fixed}
            onChange={() => {
              setFixed(false);
              setSaved(false);
            }}
          />
          {strings.memory.perKind}
        </label>
        <label className={forms.choice}>
          <input
            type="radio"
            name="default-memory"
            checked={fixed}
            onChange={() => {
              setFixed(true);
              setSaved(false);
            }}
          />
          {strings.memory.fixed}
        </label>
      </fieldset>
      {!fixed && <p className={styles.hint}>{strings.memory.perKindHint}</p>}
      {fixed && (
        <>
          <Field
            label={d.memory}
            hint={
              policy
                ? d.memoryHint({
                    min: policy.minHeapMib,
                    max: policy.maxHeapMib,
                  })
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
        </>
      )}
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
      <label className={forms.choice}>
        <input
          type="checkbox"
          checked={fullscreen}
          disabled={disabled}
          onChange={(event) => {
            setFullscreen(event.target.checked);
            setSaved(false);
          }}
        />
        {d.fullscreen}
      </label>
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
