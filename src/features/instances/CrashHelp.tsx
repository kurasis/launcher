import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StatusBadge } from '../../components/StatusBadge';
import type {
  CrashAction,
  CrashDiagnosis,
  GameFolder,
  InstanceSummary,
  ModsFix,
  RunSummary,
} from '../../lib/contracts';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { crashText } from '../diagnostics/crashText';
import { AddDialog } from '../mods/AddDialog';
import { ExplanationFeedback } from '../reports/ExplanationFeedback';
import { ReportDialog } from '../reports/ReportDialog';
import { openReview } from '../reviews/openReview';
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
  const [reporting, setReporting] = useState(false);
  const text = crashText(diagnosis);
  const error = openFolder.error ?? repair.error;
  // The mod the game needs, from the catalog (D-072): offered once it is found.
  const needed = diagnosis.actions.includes('update_mod')
    ? (diagnosis.details.dependencyId ?? null)
    : null;
  const key = needed === null ? null : `${instance.id} ${needed}`;
  const [found, setFound] = useState<{ key: string; fix: ModsFix | null } | null>(null);
  const fix = found !== null && found.key === key ? found.fix : null;
  const [adding, setAdding] = useState<ModsFix | null>(null);
  useEffect(() => {
    if (needed === null || key === null) return;
    let live = true;
    ipc
      .modsFix(instance.id, needed)
      .then((answer) => {
        if (live) setFound({ key, fix: answer });
      })
      .catch(() => {
        // Offline or the catalog does not answer: the other actions stay.
      });
    return () => {
      live = false;
    };
  }, [instance.id, needed, key]);
  const shown = diagnosis.actions.filter((action) => action !== 'update_mod' || fix !== null);

  const act = (action: CrashAction) => {
    const folder = folders[action];
    if (folder !== undefined) void openFolder.run(folder);
    else if (action === 'repair')
      void repair.run().then((started) => {
        if (started !== undefined) setRepairing(true);
      });
    else if (action === 'backups') onOpenBackups();
    else if (action === 'update_mod') setAdding(fix);
    else onEditSettings();
  };

  return (
    // Inside the installation's section, under its name (h3).
    <Panel
      title={t.title}
      level={4}
      asideAtTitle
      aside={<StatusBadge tone="danger" label={text.title} dot={false} />}
    >
      {/* Run-in headings: "Cause. …", "What to do. …". */}
      <div className={styles.runIn}>
        <h5>{t.cause}</h5> <p>{text.cause}</p>
      </div>
      <div className={styles.runIn}>
        <h5>{t.fix}</h5> <p>{text.fix}</p>
      </div>
      {diagnosis.suspects.length > 0 && (
        <p className={styles.muted} dir="auto">
          {t.suspects({ mods: diagnosis.suspects.join(', ') })}
        </p>
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
      {/* One row: the first fix is the main action, then the others, then telling us. */}
      <div className={styles.actions}>
        {shown.map((action, index) => (
          <Button
            key={action}
            variant={index === 0 ? 'primary' : 'secondary'}
            size="small"
            onClick={() => {
              act(action);
            }}
            disabled={
              (action === 'repair' && (busy || repair.pending || repairing)) ||
              (folders[action] !== undefined && openFolder.pending)
            }
          >
            {action === 'update_mod' && fix !== null
              ? (fix.installed === null ? t.installMod : t.updateMod)({ title: fix.title })
              : t.actions[action]}
          </Button>
        ))}
        <Button
          variant="ghost"
          size="small"
          onClick={() => {
            setReporting(true);
          }}
        >
          {strings.reports.offer}
        </Button>
        <Button
          variant="ghost"
          size="small"
          onClick={() => {
            openReview({
              kind: 'bug',
              category: 'launch',
              subcategory: 'crashes',
              instanceId: instance.id,
              runId: run.runId,
            });
          }}
        >
          {t.tellMore}
        </Button>
      </div>
      {repairing && <Notice tone="info">{t.repairStarted}</Notice>}
      {error !== null && <Notice tone="danger">{error}</Notice>}
      <ExplanationFeedback key={run.runId} diagnosis={diagnosis} />
      {adding !== null && (
        <AddDialog
          target={{
            instance,
            versionId: adding.versionId,
            title: adding.title,
            kind: 'mod',
            replaces: adding.replaces,
            world: null,
          }}
          onClose={() => {
            setAdding(null);
          }}
          onDone={() => {
            setFound(null);
          }}
        />
      )}
      {reporting && (
        <ReportDialog
          subject={{ type: 'game', runId: run.runId, instanceId: instance.id }}
          onClose={() => {
            setReporting(false);
          }}
        />
      )}
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
