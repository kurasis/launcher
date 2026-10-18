import { useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { AppSnapshot, SettingsPatch, UpdateMode } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './Updates.module.css';

const t = strings.updates.settings;
const modes: UpdateMode[] = ['auto', 'notify', 'off'];

/** Settings → General → Updates (D-063). */
export function UpdateSettings({ snapshot }: { snapshot: AppSnapshot }) {
  const { settings, update } = snapshot;
  const save = useCommand((requestId: string, patch: SettingsPatch) =>
    ipc.setAppSettings(patch, requestId),
  );
  const check = useCommand(() => ipc.checkForUpdate());
  const [checked, setChecked] = useState(false);
  if (!update.configured) {
    return <p className={styles.muted}>{t.notConfigured}</p>;
  }
  return (
    <fieldset className={forms.choices} disabled={save.pending || !settings.writable}>
      <legend>{t.title}</legend>
      {modes.map((mode) => (
        <label key={mode} className={forms.choice}>
          <input
            type="radio"
            name="updateMode"
            value={mode}
            checked={settings.updateMode === mode}
            onChange={() => {
              void save.run({ updateMode: mode });
            }}
          />
          {t.modes[mode]}
        </label>
      ))}
      <p className={styles.muted}>{t.current({ version: snapshot.appVersion })}</p>
      <div className={styles.actions}>
        <Button
          disabled={check.pending}
          onClick={() => {
            setChecked(false);
            void check.run().then((view) => {
              if (view !== undefined) setChecked(true);
            });
          }}
        >
          {check.pending ? t.checking : t.check}
        </Button>
      </div>
      {checked && update.phase === 'idle' && <p role="status">{t.upToDate}</p>}
      {save.error !== null && <Notice tone="danger">{save.error}</Notice>}
      {check.error !== null && <Notice tone="danger">{check.error}</Notice>}
    </fieldset>
  );
}
