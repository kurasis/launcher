import { useId } from 'react';
import { Choice } from '../../components/Choice';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import type { DownloadSource, SettingsPatch, SettingsView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './Network.module.css';
import { sourceNames } from './sourceNames';

const t = strings.mirror;
const choices: DownloadSource[] = ['automatic', 'mirror_first', 'origin_only'];

/** Where downloads come from: origins, the launcher's mirror, or both (D-062). */
export function DownloadSourceSettings({
  settings,
  mirroredHosts,
}: {
  settings: SettingsView;
  mirroredHosts: string[];
}) {
  const update = useCommand((requestId: string, patch: SettingsPatch) =>
    ipc.setAppSettings(patch, requestId),
  );
  const mirrored = sourceNames(mirroredHosts);
  const headingId = useId();

  return (
    <Panel title={t.title} level={3} headingId={headingId}>
      <fieldset
        className={styles.choices}
        aria-labelledby={headingId}
        disabled={!settings.writable}
        aria-busy={update.pending || undefined}
      >
        {choices.map((choice) => (
          <Choice
            key={choice}
            type="radio"
            name="downloadSource"
            value={choice}
            checked={settings.downloadSource === choice}
            title={t.sources[choice]}
            hint={t.hints[choice]}
            onChange={() => {
              // A change being saved: the choices wait for it but keep the focus.
              if (!update.pending) void update.run({ downloadSource: choice });
            }}
          />
        ))}
      </fieldset>
      <p className={styles.hint}>{t.checked}</p>
      {mirrored.length > 0 && (
        <Notice tone="info">{t.inUse({ sites: mirrored.join(', ') })}</Notice>
      )}
      {update.error !== null && <Notice tone="danger">{update.error}</Notice>}
    </Panel>
  );
}
