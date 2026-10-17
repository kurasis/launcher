import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StatusBadge } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type { InstanceSummary } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { healthTone } from './healthTone';
import styles from './Instances.module.css';

interface InstanceDetailsProps {
  instance: InstanceSummary;
  onEditSettings: () => void;
}

const t = strings.details;

/** Facts about one installation; its settings are edited on the Settings screen. */
export function InstanceDetails({ instance, onEditSettings }: InstanceDetailsProps) {
  const openFolder = useCommand((requestId: string, id: string) =>
    ipc.openInstanceFolder(id, requestId),
  );
  const last = instance.lastResult;
  const java = instance.java;

  return (
    <Panel
      title={t.summaryTitle}
      aside={
        <StatusBadge
          tone={healthTone[instance.health]}
          label={strings.instances.health[instance.health]}
        />
      }
    >
      <dl className={styles.facts}>
        <dt>{t.version}</dt>
        <dd>Minecraft {instance.versionId}</dd>
        <dt>{t.java}</dt>
        <dd>
          {java === null
            ? t.javaNone
            : strings.settings.javaValue(
                strings.settings.javaShort[java.provider],
                java.major,
                java.release,
              )}
        </dd>
        <dt>{t.window}</dt>
        <dd>
          {t.windowValue(
            instance.maxHeapMib,
            instance.resolution.width,
            instance.resolution.height,
          )}
        </dd>
        <dt>{t.lastResult}</dt>
        <dd>
          {last === null
            ? t.noResult
            : `${strings.diagnostics.kind[last.kind]}: ${strings.diagnostics.outcome[last.outcome]}${
                last.exitCode !== undefined
                  ? ` (${strings.runs.exitCode.toLowerCase()} ${String(last.exitCode)})`
                  : ''
              } · ${new Date(last.finishedAt).toLocaleString()}`}
        </dd>
      </dl>
      <div className={forms.actions}>
        <Button onClick={onEditSettings}>{t.edit}</Button>
        <Button onClick={() => void openFolder.run(instance.id)} disabled={openFolder.pending}>
          {t.openFolder}
        </Button>
      </div>
      <p className={styles.muted}>{t.openFolderHint}</p>
      {openFolder.error !== null && <Notice tone="danger">{openFolder.error}</Notice>}
    </Panel>
  );
}
