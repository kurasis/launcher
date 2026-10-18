import { useState } from 'react';
import { Button } from '../../components/Button';
import { Field } from '../../components/Field';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StatusBadge } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type { InstanceSummary } from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { numberLocale, strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { DeleteInstance } from './DeleteInstance';
import { healthTone } from './healthTone';
import styles from './Instances.module.css';
import { gameLabel } from './loaderLabel';
import { javaLabel } from '../settings/javaLabel';

interface InstanceDetailsProps {
  instance: InstanceSummary;
  onEditSettings: () => void;
  /** Something runs, so the installation cannot be deleted now. */
  busy?: boolean;
  onDeleted?: (message: string) => void;
}

const t = strings.details;

/** Facts about one installation; its settings are edited on the Settings screen. */
export function InstanceDetails({
  instance,
  onEditSettings,
  busy = false,
  onDeleted = () => undefined,
}: InstanceDetailsProps) {
  const openFolder = useCommand((requestId: string, id: string) =>
    ipc.openInstanceFolder(id, requestId),
  );
  const last = instance.lastResult;

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
        <dd>{gameLabel(instance)}</dd>
        <dt>{t.java}</dt>
        <dd>{javaLabel(instance)}</dd>
        {instance.jvmArguments !== '' && (
          <>
            <dt>{t.jvmArguments}</dt>
            <dd dir="ltr">{instance.jvmArguments}</dd>
          </>
        )}
        <dt>{t.window}</dt>
        <dd>
          {t.windowValue({
            mib: instance.maxHeapMib,
            width: instance.resolution.width,
            height: instance.resolution.height,
          })}
        </dd>
        <dt>{t.lastResult}</dt>
        <dd>
          {last === null
            ? t.noResult
            : `${strings.diagnostics.kind[last.kind]}: ${strings.diagnostics.outcome[last.outcome]}${
                last.exitCode !== undefined
                  ? ` (${strings.runs.exitCode.toLowerCase()} ${String(last.exitCode)})`
                  : ''
              } · ${new Date(last.finishedAt).toLocaleString(numberLocale())}`}
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
      <NotesEditor key={instance.id} instance={instance} />
      <DeleteInstance instance={instance} busy={busy} onDeleted={onDeleted} />
    </Panel>
  );
}

/** The installation's notes; saved on request, also while it is busy. */
function NotesEditor({ instance }: { instance: InstanceSummary }) {
  const [draft, setDraft] = useState(instance.notes);
  const [saved, setSaved] = useState(false);
  const save = useCommand((requestId: string, notes: string) =>
    ipc.setInstanceNotes({ instanceId: instance.id, notes }, requestId),
  );
  const changed = draft.trim() !== instance.notes;
  return (
    <form
      className={forms.form}
      onSubmit={(event) => {
        event.preventDefault();
        void save.run(draft).then((result) => {
          if (result) setSaved(true);
        });
      }}
    >
      <Field label={t.notes} hint={t.notesHint}>
        {(props) => (
          <textarea
            {...props}
            className={[forms.input, styles.notes].join(' ')}
            value={draft}
            maxLength={4000}
            rows={4}
            onChange={(event) => {
              setDraft(event.target.value);
              setSaved(false);
            }}
          />
        )}
      </Field>
      <div className={forms.actions}>
        <Button type="submit" disabled={save.pending || !changed}>
          {t.notesSave}
        </Button>
        {saved && !changed && <span role="status">{t.saved}</span>}
      </div>
      {save.error !== null && <Notice tone="danger">{save.error}</Notice>}
    </form>
  );
}
