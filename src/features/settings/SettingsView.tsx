import { type KeyboardEvent, useId, useRef, useState } from 'react';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { Screen } from '../../components/Screen';
import { Segmented } from '../../components/Segmented';
import forms from '../../components/forms.module.css';
import type {
  AppSnapshot,
  DuringGameMode,
  SettingsPatch,
  StartView,
  Theme,
} from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { LANGUAGES, type SettingsSection, strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { DefaultsForm } from './DefaultsForm';
import { InstallationForm } from './InstallationForm';
import { InstanceJavaForm } from './InstanceJavaForm';
import { InstanceLoaderForm } from './InstanceLoaderForm';
import { DataFolderSettings } from './DataFolderSettings';
import { JavaSettings } from './JavaSettings';
import { PrivacySettings } from './PrivacySettings';
import { UpdateSettings } from '../updates/UpdateSettings';
import { DownloadSourceSettings } from '../network/DownloadSourceSettings';
import { NetworkSettings } from '../network/NetworkSettings';
import { ResetSettings } from './ResetSettings';
import { FeedSettings } from '../feed/FeedSettings';
import styles from './SettingsView.module.css';
import { gameLabel } from '../instances/loaderLabel';

interface SettingsViewProps {
  snapshot: AppSnapshot;
  section: SettingsSection;
  onSection: (section: SettingsSection) => void;
  /** Installation to show first in the Installations section. */
  instanceId?: string | null;
}

const t = strings.settings;
const sections: SettingsSection[] = [
  'general',
  'feed',
  'java',
  'defaults',
  'network',
  'installations',
  'privacy',
  'reset',
];
const themes: Theme[] = ['system', 'light', 'dark'];
const modes: DuringGameMode[] = ['unload_and_return', 'keep_open', 'close_launcher'];
/** Every start screen, also those the interface does not have yet (they open Installations). */
const startViews: readonly StartView[] = ['feed', 'home', 'mods', 'skins', 'account', 'last'];

/**
 * Every launcher and installation setting, one section at a time (concept A): the
 * sections down the inline start, the chosen one's hint and panels beside them.
 */
export function SettingsView({ snapshot, section, onSection, instanceId }: SettingsViewProps) {
  const { settings } = snapshot;
  const panelId = useId();
  const tabs = useRef(new Map<SettingsSection, HTMLButtonElement>());

  // The arrows move the focus along the sections; Enter or Space opens one. Opening
  // on every arrow would load each section on the way and drop what was typed.
  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const rtl = getComputedStyle(event.currentTarget).direction === 'rtl';
    const steps: Record<string, number> = {
      ArrowDown: 1,
      ArrowUp: -1,
      ArrowRight: rtl ? -1 : 1,
      ArrowLeft: rtl ? 1 : -1,
    };
    let next: number;
    if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = sections.length - 1;
    else {
      const step = steps[event.key];
      if (step === undefined) return;
      next = (index + step + sections.length) % sections.length;
    }
    event.preventDefault();
    const item = sections[next];
    if (item !== undefined) tabs.current.get(item)?.focus();
  };

  return (
    <Screen title={t.title}>
      <div className={styles.layout}>
        <div
          role="tablist"
          aria-label={t.sectionsLabel}
          aria-orientation="vertical"
          className={styles.subnav}
        >
          {sections.map((item, index) => (
            <button
              key={item}
              ref={(node) => {
                if (node === null) tabs.current.delete(item);
                else tabs.current.set(item, node);
              }}
              type="button"
              role="tab"
              id={`${panelId}-${item}`}
              aria-selected={section === item}
              aria-controls={panelId}
              tabIndex={section === item ? 0 : -1}
              className={styles.tab}
              onClick={() => {
                onSection(item);
              }}
              onKeyDown={(event) => {
                onKeyDown(event, index);
              }}
            >
              {t.sections[item]}
            </button>
          ))}
        </div>
        <div
          role="tabpanel"
          id={panelId}
          aria-labelledby={`${panelId}-${section}`}
          className={styles.content}
        >
          <p className={styles.hint}>{t.sectionHints[section]}</p>
          {!settings.writable && <Notice tone="warning">{t.readOnly}</Notice>}
          {section === 'general' && <GeneralSettings snapshot={snapshot} />}
          {section === 'feed' && <FeedSettings settings={settings} />}
          {section === 'java' && (
            <JavaSettings settings={settings} instances={snapshot.instances} />
          )}
          {section === 'defaults' && <DefaultsForm settings={settings} memory={snapshot.memory} />}
          {section === 'network' && (
            <>
              <NetworkSettings snapshot={snapshot} />
              <DownloadSourceSettings settings={settings} mirroredHosts={snapshot.mirroredHosts} />
            </>
          )}
          {section === 'privacy' && <PrivacySettings settings={settings} />}
          {section === 'reset' && <ResetSettings busy={snapshot.activeOperation !== null} />}
          {section === 'installations' && (
            <InstallationsSettings
              snapshot={snapshot}
              initial={instanceId ?? settings.selectedInstanceId}
            />
          )}
        </div>
      </div>
    </Screen>
  );
}

function GeneralSettings({ snapshot }: { snapshot: AppSnapshot }) {
  const { settings } = snapshot;
  const update = useCommand((requestId: string, patch: SettingsPatch) =>
    ipc.setAppSettings(patch, requestId),
  );
  // Saving takes a moment: the controls wait for it but keep the focus.
  const pending = update.pending;
  const save = (patch: SettingsPatch) => {
    if (!pending) void update.run(patch);
  };
  const disabled = !settings.writable;

  return (
    <>
      <Panel>
        <div className={styles.row}>
          <Field label={t.language}>
            {(props) => (
              <select
                {...props}
                className={[forms.select, styles.wide].join(' ')}
                value={settings.language}
                disabled={disabled}
                aria-busy={pending || undefined}
                onChange={(event) => {
                  save({ language: event.target.value });
                }}
              >
                {/* A setting naming a language this build lacks still shows. */}
                {!LANGUAGES.some((l) => l.tag === settings.language) && (
                  <option value={settings.language}>{settings.language}</option>
                )}
                {LANGUAGES.map((item) => (
                  <option key={item.tag} value={item.tag} lang={item.tag} dir={item.dir}>
                    {item.translated < 100
                      ? `${item.name} · ${t.languageIncomplete({ percent: item.translated })}`
                      : item.name}
                  </option>
                ))}
              </select>
            )}
          </Field>
          <Field label={t.startView} hint={t.startViewHint}>
            {(props) => (
              <select
                {...props}
                className={[forms.select, styles.wide].join(' ')}
                value={settings.startView}
                disabled={disabled}
                aria-busy={pending || undefined}
                onChange={(event) => {
                  const startView = startViews.find((item) => item === event.target.value);
                  if (startView !== undefined) save({ startView });
                }}
              >
                {startViews.map((item) => (
                  <option key={item} value={item}>
                    {t.startViews[item]}
                  </option>
                ))}
              </select>
            )}
          </Field>
        </div>
        <div className={styles.field}>
          <span className={styles.label}>{t.theme}</span>
          <Segmented
            label={t.theme}
            options={themes.map((theme) => ({ value: theme, label: t.themes[theme] }))}
            value={settings.theme}
            disabled={disabled}
            pending={pending}
            onChange={(theme) => {
              save({ theme });
            }}
          />
        </div>
        <fieldset className={forms.choices} disabled={disabled} aria-busy={pending || undefined}>
          <legend>{t.duringGame}</legend>
          {modes.map((mode) => (
            <label key={mode} className={forms.choice}>
              <input
                type="radio"
                name="duringGameMode"
                value={mode}
                checked={settings.duringGameMode === mode}
                onChange={() => {
                  save({ duringGameMode: mode });
                }}
              />
              {t.duringGameModes[mode]}
            </label>
          ))}
        </fieldset>
        {update.error !== null && <Notice tone="danger">{update.error}</Notice>}
      </Panel>
      <DataFolderSettings busy={snapshot.activeOperation !== null} />
      <UpdateSettings snapshot={snapshot} />
    </>
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
  const instance = instances.find((i) => i.id === chosen) ?? instances[0];
  if (instance === undefined) {
    return (
      <Panel>
        <p className={styles.hint}>{t.noInstallations}</p>
      </Panel>
    );
  }

  return (
    <>
      <Panel>
        <Field label={t.installation}>
          {(props) => (
            <select
              {...props}
              className={[forms.select, styles.long].join(' ')}
              value={instance.id}
              onChange={(event) => {
                setChosen(event.target.value);
              }}
            >
              {instances.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.displayName} · {gameLabel(item)}
                </option>
              ))}
            </select>
          )}
        </Field>
        <InstallationForm key={instance.id} instance={instance} memory={snapshot.memory} />
      </Panel>
      {instance.loader !== null && (
        <InstanceLoaderForm
          key={`loader-${instance.id}`}
          instance={instance}
          loader={instance.loader}
          queued={snapshot.queue.some((q) => q.instanceId === instance.id)}
        />
      )}
      <InstanceJavaForm key={`java-${instance.id}`} instance={instance} />
    </>
  );
}
