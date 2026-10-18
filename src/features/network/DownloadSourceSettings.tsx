import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
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

  return (
    <fieldset className={forms.choices} disabled={update.pending || !settings.writable}>
      <legend>{t.title}</legend>
      {choices.map((choice) => (
        <label key={choice} className={styles.check}>
          <input
            type="radio"
            name="downloadSource"
            value={choice}
            checked={settings.downloadSource === choice}
            onChange={() => {
              void update.run({ downloadSource: choice });
            }}
          />
          <span>
            <span className={styles.strong}>{t.sources[choice]}</span>
            <span className={styles.hint}>{t.hints[choice]}</span>
          </span>
        </label>
      ))}
      <p className={styles.hint}>{t.checked}</p>
      {mirrored.length > 0 && (
        <Notice tone="info">{t.inUse({ sites: mirrored.join(', ') })}</Notice>
      )}
      {update.error !== null && <Notice tone="danger">{update.error}</Notice>}
    </fieldset>
  );
}
