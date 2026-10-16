import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import forms from '../../components/forms.module.css';
import type { AppSnapshot, DuringGameMode, SettingsPatch, Theme } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './SettingsView.module.css';

interface SettingsViewProps {
  snapshot: AppSnapshot;
}

const t = strings.settings;
const themes: Theme[] = ['system', 'light', 'dark'];
const modes: DuringGameMode[] = ['unload_and_return', 'keep_open'];

export function SettingsView({ snapshot }: SettingsViewProps) {
  const { settings, dataRoot } = snapshot;
  const update = useCommand((requestId: string, patch: SettingsPatch) =>
    ipc.setAppSettings(patch, requestId),
  );
  const disabled = update.pending || !settings.writable;

  return (
    <Panel title={t.title}>
      {!settings.writable && <Notice tone="warning">{t.readOnly}</Notice>}
      <fieldset className={forms.choices} disabled={disabled}>
        <legend>{t.theme}</legend>
        {themes.map((theme) => (
          <label key={theme} className={forms.choice}>
            <input
              type="radio"
              name="theme"
              value={theme}
              checked={settings.theme === theme}
              onChange={() => {
                void update.run({ theme });
              }}
            />
            {t.themes[theme]}
          </label>
        ))}
      </fieldset>
      <fieldset className={forms.choices} disabled={disabled}>
        <legend>{t.duringGame}</legend>
        {modes.map((mode) => (
          <label key={mode} className={forms.choice}>
            <input
              type="radio"
              name="duringGameMode"
              value={mode}
              checked={settings.duringGameMode === mode}
              onChange={() => {
                void update.run({ duringGameMode: mode });
              }}
            />
            {t.duringGameModes[mode]}
          </label>
        ))}
      </fieldset>
      {update.error !== null && <Notice tone="danger">{update.error}</Notice>}
      <div>
        <h3 className={styles.subheading}>{t.dataLocation}</h3>
        <p className={styles.path}>{dataRoot.path ?? '—'}</p>
      </div>
    </Panel>
  );
}
