import { useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StatusBadge } from '../../components/StatusBadge';
import forms from '../../components/forms.module.css';
import type {
  CrashAction,
  CrashDiagnosis,
  GameFolder,
  InstanceSummary,
  RunSummary,
} from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { crashText } from '../diagnostics/crashText';
import styles from './CrashHelp.module.css';

const t = strings.crash;

const folders: Partial<Record<CrashAction, GameFolder | null>> = {
  open_mods: 'mods',
  open_config: 'config',
  open_crash_reports: 'crash-reports',
  open_game_folder: null,
};

interface CrashHelpProps {
  instance: InstanceSummary;
  run: RunSummary & { diagnosis: CrashDiagnosis };
  /** Opens this installation's settings (memory, Java, loader). */
  onEditSettings: () => void;
  onOpenBackups: () => void;
  /** Something runs, so a repair cannot start now. */
  busy: boolean;
}

/** Why the last game of this installation stopped and what to do (D-059). */
export function CrashHelp({ instance, run, onEditSettings, onOpenBackups, busy }: CrashHelpProps) {
  const diagnosis = run.diagnosis;
  const openFolder = useCommand((requestId: string, folder: GameFolder | null) =>
    ipc.openInstanceFolder(instance.id, requestId, folder),
  );
  const repair = useCommand((requestId: string) => ipc.repairInstance(instance.id, requestId));
  const [repairing, setRepairing] = useState(false);
  const text = crashText(diagnosis);
  const error = openFolder.error ?? repair.error;

  const act = (action: CrashAction) => {
    const folder = folders[action];
    if (folder !== undefined) void openFolder.run(folder);
    else if (action === 'repair')
      void repair.run().then((started) => {
        if (started !== undefined) setRepairing(true);
      });
    else if (action === 'backups') onOpenBackups();
    else onEditSettings();
  };

  return (
    <Panel title={t.title} aside={<StatusBadge tone="danger" label={text.title} />}>
      <div className={styles.section}>
        <h3 className={styles.heading}>{t.cause}</h3>
        <p>{text.cause}</p>
      </div>
      <div className={styles.section}>
        <h3 className={styles.heading}>{t.fix}</h3>
        <p>{text.fix}</p>
      </div>
      {diagnosis.suspects.length > 0 && (
        <p dir="auto">{t.suspects({ mods: diagnosis.suspects.join(', ') })}</p>
      )}
      {diagnosis.notes.map((note, index) => {
        const found = crashText(note);
        return (
          <Notice key={index} tone="info">
            <p>
              <strong>
                {t.notes}: {found.title}
              </strong>
            </p>
            <p>{found.cause}</p>
            <p>{found.fix}</p>
          </Notice>
        );
      })}
      <p className={styles.muted}>
        {[
          run.exitCode !== null && t.exitCode({ code: run.exitCode }),
          run.signal !== null && t.signal({ signal: run.signal }),
          diagnosis.report !== null && t.report({ file: diagnosis.report }),
        ]
          .filter(Boolean)
          .join(' · ')}
      </p>
      {diagnosis.actions.length > 0 && (
        <div className={forms.actions}>
          {diagnosis.actions.map((action) => (
            <Button
              key={action}
              onClick={() => {
                act(action);
              }}
              disabled={
                (action === 'repair' && (busy || repair.pending || repairing)) ||
                (folders[action] !== undefined && openFolder.pending)
              }
            >
              {t.actions[action]}
            </Button>
          ))}
        </div>
      )}
      {repairing && <Notice tone="info">{t.repairStarted}</Notice>}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      {diagnosis.evidence.length > 0 && (
        <details>
          <summary>{t.evidence}</summary>
          <pre className={styles.evidence} dir="ltr">
            {diagnosis.evidence.join('\n')}
          </pre>
        </details>
      )}
    </Panel>
  );
}
