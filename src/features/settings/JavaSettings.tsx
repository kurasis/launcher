import { useId } from 'react';
import { Choice } from '../../components/Choice';
import { Fact, Facts } from '../../components/Facts';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
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
  const headingId = useId();

  return (
    <>
      <Panel title={t.javaProvider} level={3} headingId={headingId}>
        <fieldset
          className={forms.choices}
          aria-labelledby={headingId}
          disabled={!settings.writable}
          aria-busy={update.pending || undefined}
        >
          {providers.map((provider) => (
            <Choice
              key={provider}
              type="radio"
              name="javaProvider"
              value={provider}
              checked={settings.javaProvider === provider}
              title={t.javaProviders[provider]}
              hint={t.javaProviderHints[provider]}
              onChange={() => {
                // A change being saved: the choices wait for it but keep the focus.
                if (!update.pending) void update.run(provider);
              }}
            />
          ))}
        </fieldset>
        <p className={styles.hint}>{t.javaApplies}</p>
        {update.error !== null && <Notice tone="danger">{update.error}</Notice>}
      </Panel>
      <Panel title={t.javaInstalled} level={3}>
        {instances.length === 0 ? (
          <p className={styles.hint}>{t.noInstallations}</p>
        ) : (
          <Facts>
            {instances.map((instance) => (
              <Fact key={instance.id} term={<bdi>{instance.displayName}</bdi>}>
                {javaLabel(instance)}
              </Fact>
            ))}
          </Facts>
        )}
      </Panel>
    </>
  );
}
