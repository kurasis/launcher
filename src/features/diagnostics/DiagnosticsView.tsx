import { useState } from 'react';
import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import { StatusBadge, type StatusTone } from '../../components/StatusBadge';
import type { AppSnapshot, Outcome } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { LifecycleHarnessPanel } from './LifecycleHarnessPanel';
import { RunStatus } from './RunStatus';
import styles from './RunStatus.module.css';
import { localizeError } from '../../lib/i18n/errors';

const t = strings.diagnostics;

const outcomeTone: Record<Outcome, StatusTone> = {
  succeeded: 'success',
  failed: 'danger',
  cancelled: 'neutral',
  interrupted: 'warning',
};

/** Recent results, the last run's output and a user-requested redacted export. */
export function DiagnosticsView({ snapshot }: { snapshot: AppSnapshot }) {
  const [message, setMessage] = useState<string | null>(null);
  const exporter = useCommand((requestId: string, runId: string | null) =>
    ipc.exportDiagnostics(runId, requestId),
  );
  const names = new Map(snapshot.instances.map((i) => [i.id, i.displayName]));

  const runExport = async () => {
    setMessage(null);
    const result = await exporter.run(snapshot.lastRun?.runId ?? null);
    if (result === undefined) return;
    setMessage(
      result.saved && result.fileName !== null ? t.exported(result.fileName) : t.exportCancelled,
    );
  };

  return (
    <>
      <Panel
        title={strings.console.title}
        aside={<Button onClick={() => void ipc.openConsole()}>{strings.console.open}</Button>}
      >
        <p className={styles.muted}>{strings.console.openHint}</p>
      </Panel>
      <RunStatus current={snapshot.currentRun} last={snapshot.lastRun} />
      <Panel title={t.recentTitle}>
        {snapshot.recentOperations.length === 0 ? (
          <p className={styles.muted}>{t.none}</p>
        ) : (
          <table className={styles.table}>
            <thead>
              <tr>
                <th scope="col">{t.columns.operation}</th>
                <th scope="col">{t.columns.installation}</th>
                <th scope="col">{t.columns.result}</th>
                <th scope="col">{t.columns.transferred}</th>
              </tr>
            </thead>
            <tbody>
              {snapshot.recentOperations.map((op) => (
                <tr key={op.operationId}>
                  <td>{t.kind[op.kind]}</td>
                  <td>{op.instanceId === null ? '—' : (names.get(op.instanceId) ?? '—')}</td>
                  <td>
                    <StatusBadge tone={outcomeTone[op.outcome]} label={t.outcome[op.outcome]} />
                    {op.error !== null && (
                      <span className={styles.muted}> {localizeError(op.error)}</span>
                    )}
                  </td>
                  <td>{formatBytes(op.networkBytes)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Panel>
      <Panel title={t.exportTitle}>
        <p>{t.exportExplanation}</p>
        <p className={styles.muted}>{t.exportExcluded}</p>
        <div>
          <Button variant="primary" disabled={exporter.pending} onClick={() => void runExport()}>
            {t.exportButton}
          </Button>
        </div>
        {message !== null && <p role="status">{message}</p>}
        {exporter.error !== null && <Notice tone="danger">{exporter.error}</Notice>}
      </Panel>
      {snapshot.lifecycleHarnessAvailable && <LifecycleHarnessPanel snapshot={snapshot} />}
    </>
  );
}
