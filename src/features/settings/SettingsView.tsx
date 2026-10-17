import { useId, useState } from 'react';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import forms from '../../components/forms.module.css';
import type { AppSnapshot, DuringGameMode, SettingsPatch, Theme } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { type SettingsSection, strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { DefaultsForm } from './DefaultsForm';
import { InstallationForm } from './InstallationForm';
import { JavaSettings } from './JavaSettings';
import styles from './SettingsView.module.css';

interface SettingsViewProps {
  snapshot: AppSnapshot;
  section: SettingsSection;
  onSection: (section: SettingsSection) => void;
  /** Installation to show first in the Installations section. */
  instanceId?: string | null;
}

const t = strings.settings;
const sections: SettingsSection[] = ['general', 'java', 'defaults', 'installations'];
const themes: Theme[] = ['system', 'light', 'dark'];
const modes: DuringGameMode[] = ['unload_and_return', 'keep_open'];

/** Every launcher and installation setting, one section at a time. */
export function SettingsView({ snapshot, section, onSection, instanceId }: SettingsViewProps) {
  const { settings } = snapshot;
  const panelId = useId();

  return (
    <div className={styles.layout}>
      <div role="tablist" aria-label={t.sectionsLabel} className={styles.sections}>
        {sections.map((item) => (
          <button
            key={item}
            type="button"
            role="tab"
            id={`${panelId}-${item}`}
            aria-selected={section === item}
            aria-controls={panelId}
            className={styles.sectionTab}
            onClick={() => {
              onSection(item);
            }}
          >
            {t.sections[item]}
          </button>
        ))}
      </div>
      <div role="tabpanel" id={panelId} aria-labelledby={`${panelId}-${section}`}>
        <Panel title={t.sections[section]}>
          <p className={styles.hint}>{t.sectionHints[section]}</p>
          {!settings.writable && <Notice tone="warning">{t.readOnly}</Notice>}
          {section === 'general' && <GeneralSettings snapshot={snapshot} />}
          {section === 'java' && (
            <JavaSettings settings={settings} instances={snapshot.instances} />
          )}
          {section === 'defaults' && <DefaultsForm settings={settings} memory={snapshot.memory} />}
          {section === 'installations' && (
            <InstallationsSettings
              snapshot={snapshot}
              initial={instanceId ?? settings.selectedInstanceId}
            />
          )}
        </Panel>
      </div>
    </div>
  );
}

function GeneralSettings({ snapshot }: { snapshot: AppSnapshot }) {
  const { settings, dataRoot } = snapshot;
  const update = useCommand((requestId: string, patch: SettingsPatch) =>
    ipc.setAppSettings(patch, requestId),
  );
  const disabled = update.pending || !settings.writable;

  return (
    <div className={styles.section}>
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
        <p className={styles.hint}>{t.dataLocationHint}</p>
      </div>
    </div>
  );
}

function InstallationsSettings({
  snapshot,
  initial,
}: {
  snapshot: AppSnapshot;
  initial: string | null;
}) {
  const { instances } = snapshot;
  const [chosen, setChosen] = useState(initial);
  const selectId = useId();
  const instance = instances.find((i) => i.id === chosen) ?? instances[0];
  if (instance === undefined) return <p className={styles.hint}>{t.noInstallations}</p>;

  return (
    <div className={styles.section}>
      <div className={styles.field}>
        <label htmlFor={selectId}>{t.installation}</label>
        <select
          id={selectId}
          className={forms.select}
          value={instance.id}
          onChange={(event) => {
            setChosen(event.target.value);
          }}
        >
          {instances.map((item) => (
            <option key={item.id} value={item.id}>
              {item.displayName} · Minecraft {item.versionId}
            </option>
          ))}
        </select>
      </div>
      <InstallationForm key={instance.id} instance={instance} memory={snapshot.memory} />
    </div>
  );
}
