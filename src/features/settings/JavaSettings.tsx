import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type { InstanceSummary, JavaProvider, SettingsView } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { javaLabel } from './javaLabel';
import styles from './SettingsView.module.css';

const t = strings.settings;
const providers: JavaProvider[] = ['mojang', 'temurin'];

/** Runtime source for new installations, and the Java each installation uses (D-036). */
export function JavaSettings({
  settings,
  instances,
}: {
  settings: SettingsView;
  instances: InstanceSummary[];
}) {
  const update = useCommand((requestId: string, javaProvider: JavaProvider) =>
    ipc.setAppSettings({ javaProvider }, requestId),
  );

  return (
    <div className={styles.section}>
      <fieldset className={forms.choices} disabled={update.pending || !settings.writable}>
        <legend>{t.javaProvider}</legend>
        {providers.map((provider) => (
          <label key={provider} className={styles.option}>
            <input
              type="radio"
              name="javaProvider"
              value={provider}
              checked={settings.javaProvider === provider}
              onChange={() => {
                void update.run(provider);
              }}
            />
            <span>
              <span className={styles.optionTitle}>{t.javaProviders[provider]}</span>
              <span className={styles.hint}>{t.javaProviderHints[provider]}</span>
            </span>
          </label>
        ))}
      </fieldset>
      <p className={styles.hint}>{t.javaApplies}</p>
      {update.error !== null && <Notice tone="danger">{update.error}</Notice>}
      <div>
        <h3 className={styles.subheading}>{t.javaInstalled}</h3>
        {instances.length === 0 ? (
          <p className={styles.hint}>{t.noInstallations}</p>
        ) : (
          <dl className={styles.facts}>
            {instances.map((instance) => (
              <div key={instance.id} className={styles.fact}>
                <dt>{instance.displayName}</dt>
                <dd>{javaLabel(instance)}</dd>
              </div>
            ))}
          </dl>
        )}
      </div>
    </div>
  );
}
