import { useId, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { Segmented } from '../../components/Segmented';
import forms from '../../components/forms.module.css';
import type { AppSnapshot, SettingsPatch, UpdateChannel, UpdateMode } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { phaseText } from './phase';
import styles from './Updates.module.css';

const t = strings.updates.settings;
const modes: UpdateMode[] = ['auto', 'notify', 'off'];
const channels: UpdateChannel[] = ['stable', 'beta'];

/** Settings → General → Updates (D-063). */
export function UpdateSettings({ snapshot }: { snapshot: AppSnapshot }) {
  const headingId = useId();
  return (
    <Panel title={t.title} level={3} headingId={headingId}>
      {snapshot.update.configured ? (
        <Updates snapshot={snapshot} headingId={headingId} />
      ) : (
        <p className={styles.muted}>{t.notConfigured}</p>
      )}
    </Panel>
  );
}

function Updates({ snapshot, headingId }: { snapshot: AppSnapshot; headingId: string }) {
  const { settings, update } = snapshot;
  const save = useCommand((requestId: string, patch: SettingsPatch) =>
    ipc.setAppSettings(patch, requestId),
  );
  const check = useCommand(() => ipc.checkForUpdate());
  const [checked, setChecked] = useState(false);
  // Shown after leaving beta: the installed version stays until stable catches up.
  const [leftBeta, setLeftBeta] = useState(false);
  // A change being saved: the choices wait for it but keep the focus.
  const store = (patch: SettingsPatch) => (save.pending ? null : save.run(patch));
  const phase = phaseText(update);

  return (
    <>
      <fieldset
        className={forms.choices}
        aria-labelledby={headingId}
        disabled={!settings.writable}
        aria-busy={save.pending || undefined}
      >
        {modes.map((mode) => (
          <label key={mode} className={forms.choice}>
            <input
              type="radio"
              name="updateMode"
              value={mode}
              checked={settings.updateMode === mode}
              onChange={() => {
                void store({ updateMode: mode });
              }}
            />
            {t.modes[mode]}
          </label>
        ))}
      </fieldset>
      <div className={styles.status}>
        <p>
          {t.current({ version: snapshot.appVersion })}
          {phase !== null && ` ${phase}`}
        </p>
        <Button
          size="small"
          aria-disabled={check.pending || undefined}
          onClick={() => {
            if (check.pending) return;
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
      <div className={styles.field}>
        <span className={styles.label}>{t.channel.title}</span>
        <Segmented
          label={t.channel.title}
          options={channels.map((channel) => ({ value: channel, label: t.channel[channel] }))}
          value={settings.updateChannel}
          disabled={!settings.writable}
          pending={save.pending}
          onChange={(channel) => {
            setLeftBeta(channel === 'stable');
            void store({ updateChannel: channel })?.then((saved) => {
              // Look on the new channel at once.
              if (saved !== undefined && settings.updateMode !== 'off') void check.run();
            });
          }}
        />
        {settings.updateChannel === 'beta' && <p className={styles.muted}>{t.channel.betaHint}</p>}
        {settings.updateChannel === 'stable' && leftBeta && (
          <p className={styles.muted}>{t.channel.stableHint}</p>
        )}
      </div>
      {save.error !== null && <Notice tone="danger">{save.error}</Notice>}
      {check.error !== null && <Notice tone="danger">{check.error}</Notice>}
    </>
  );
}
