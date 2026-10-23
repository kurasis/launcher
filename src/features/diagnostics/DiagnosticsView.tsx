import { useState } from 'react';
import { useAppInfo } from '../../app/useAppInfo';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { Screen } from '../../components/Screen';
import { StateDot, type DotTone } from '../../components/StateDot';
import type { AppSnapshot, OperationKind, Outcome } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { localizeError, numberLocale, strings } from '../../lib/strings';
import { calendarDays, daysAgo } from '../../lib/time';
import { useCommand } from '../../lib/useCommand';
import { useShownError } from '../../lib/useShownError';
import styles from './Diagnostics.module.css';
import { LifecycleHarnessPanel } from './LifecycleHarnessPanel';
import { RunStatus } from './RunStatus';
import { SystemInfoPanel } from './SystemInfoPanel';

const t = strings.diagnostics;

const outcomeTone: Record<Outcome, DotTone> = {
  succeeded: 'success',
  failed: 'danger',
  cancelled: 'neutral',
  interrupted: 'warning',
};

/** Operations shown as a row while they run: a game run has its own panel. */
const ROW_KINDS: ReadonlySet<OperationKind> = new Set([
  'install',
  'repair',
  'relocate',
  'backup',
  'restore',
]);

/** "yesterday, 19:12" for today and yesterday, "3 October" before, with the year when not this one. */
function finishedWhen(finishedAt: string, now = Date.now()): string {
  const at = Date.parse(finishedAt);
  if (Number.isNaN(at)) return '';
  const date = new Date(at);
  if (calendarDays(at, now) <= 1) {
    const time = new Intl.DateTimeFormat(numberLocale(), {
      hour: 'numeric',
      minute: '2-digit',
    }).format(date);
    return t.dayTime({ day: daysAgo(at, now), time });
  }
  return new Intl.DateTimeFormat(numberLocale(), {
    ...(date.getFullYear() === new Date(now).getFullYear() ? {} : { year: 'numeric' as const }),
    month: 'long',
    day: 'numeric',
  }).format(date);
}

/** Recent results, the last run's output and a user-requested redacted export. */
export function DiagnosticsView({ snapshot }: { snapshot: AppSnapshot }) {
  const [message, setMessage] = useState<string | null>(null);
  const opening = useShownError();
  const exporter = useCommand((requestId: string, runId: string | null) =>
    ipc.exportDiagnostics(runId, requestId, strings.windowTitles.saveDiagnostics),
  );
  const info = useAppInfo();
  const names = new Map(snapshot.instances.map((i) => [i.id, i.displayName]));
  const nameOf = (instanceId: string | null) =>
    instanceId === null ? '—' : (names.get(instanceId) ?? '—');

  // The operation under way, until its result is listed.
  const active = snapshot.activeOperation;
  const running =
    active !== null &&
    ROW_KINDS.has(active.kind) &&
    !snapshot.recentOperations.some((op) => op.operationId === active.operationId)
      ? active
      : null;
  const progress =
    running !== null && snapshot.operationProgress?.operationId === running.operationId
      ? snapshot.operationProgress
      : null;
  const hasRuns = snapshot.currentRun !== null || snapshot.lastRun !== null;

  const runExport = async () => {
    if (exporter.pending) return;
    setMessage(null);
    const result = await exporter.run(snapshot.lastRun?.runId ?? null);
    if (result === undefined) return;
    setMessage(
      result.saved && result.fileName !== null
        ? t.exported({ name: result.fileName })
        : t.exportCancelled,
    );
  };

  return (
    <Screen title={strings.nav.diagnostics}>
      <div className={styles.columns}>
        <div className={styles.column}>
          <Panel
            title={strings.console.title}
            level={3}
            action={
              <Button
                onClick={() => {
                  opening.clear();
                  void ipc.openConsole().catch(opening.catcher);
                }}
              >
                <Icon name="console" size={18} />
                {strings.console.open}
              </Button>
            }
          >
            <p className={styles.hint}>{strings.console.openHint}</p>
            {opening.error !== null && <Notice tone="danger">{opening.error}</Notice>}
          </Panel>
          <Panel title={t.recentTitle} level={3}>
            {running === null && snapshot.recentOperations.length === 0 ? (
              <p className={styles.hint}>{t.none}</p>
            ) : (
              <div className={styles.tableBox}>
                <table className={styles.table}>
                  <thead>
                    <tr>
                      <th scope="col">{t.columns.operation}</th>
                      <th scope="col">{t.columns.installation}</th>
                      <th scope="col">{t.columns.result}</th>
                      <th scope="col">{t.columns.transferred}</th>
                      <th scope="col">{t.columns.when}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {running !== null && (
                      <tr>
                        <td>{t.kind[running.kind]}</td>
                        <td>
                          <bdi>{nameOf(running.instanceId)}</bdi>
                        </td>
                        <td>
                          <StateDot tone="busy" label={t.running} />
                        </td>
                        <td className={styles.size}>
                          {progress !== null && progress.downloadedBytes > 0
                            ? formatBytes(progress.downloadedBytes)
                            : ''}
                        </td>
                        <td className={styles.when}>{t.now}</td>
                      </tr>
                    )}
                    {snapshot.recentOperations.map((op) => (
                      <tr key={op.operationId}>
                        <td>{t.kind[op.kind]}</td>
                        <td>
                          <bdi>{nameOf(op.instanceId)}</bdi>
                        </td>
                        <td>
                          <StateDot tone={outcomeTone[op.outcome]} label={t.outcome[op.outcome]} />
                          {op.error !== null && (
                            <span className={styles.error}>{localizeError(op.error)}</span>
                          )}
                        </td>
                        <td className={styles.size}>
                          {op.networkBytes > 0 ? formatBytes(op.networkBytes) : ''}
                        </td>
                        <td className={styles.when}>{finishedWhen(op.finishedAt)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </Panel>
          {/* The lifecycle test panel shows the runs itself, with a Stop button. */}
          {snapshot.lifecycleHarnessAvailable ? (
            <LifecycleHarnessPanel snapshot={snapshot} />
          ) : hasRuns ? (
            <RunStatus
              current={snapshot.currentRun}
              last={snapshot.lastRun}
              level={3}
              names={names}
              output="shown"
            />
          ) : (
            <Panel title={strings.runs.lastTitle} level={3}>
              <p className={styles.hint}>{strings.runs.none}</p>
            </Panel>
          )}
        </div>
        <div className={styles.column}>
          <Panel title={t.exportTitle} level={3}>
            <p>{t.exportExplanation}</p>
            <p className={styles.hint}>{t.exportExcluded}</p>
            <div className={styles.actions}>
              <Button
                variant="primary"
                aria-disabled={exporter.pending || undefined}
                onClick={() => void runExport()}
              >
                {t.exportButton}
              </Button>
            </div>
            {message !== null && (
              <p role="status" className={styles.hint}>
                {message}
              </p>
            )}
            {exporter.error !== null && <Notice tone="danger">{exporter.error}</Notice>}
          </Panel>
          <SystemInfoPanel
            state={info.state}
            onRetry={info.reload}
            level={3}
            memoryMib={snapshot.memory.policy?.physicalMib ?? null}
          />
        </div>
      </div>
    </Screen>
  );
}
