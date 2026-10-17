import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import forms from '../../components/forms.module.css';
import type {
  ActiveActivity,
  InstanceSummary,
  OperationProgress,
  OperationResult,
} from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './Instances.module.css';

interface InstallPanelProps {
  instance: InstanceSummary;
  activeOperation: ActiveActivity | null;
  progress: OperationProgress | null;
  recentOperations: OperationResult[];
}

const t = strings.install;

function ProgressView({ progress }: { progress: OperationProgress }) {
  const total = progress.totalDownloadBytes;
  return (
    <div className={styles.progress}>
      <p role="status" className={styles.phase}>
        {t.phase[progress.phase]}
      </p>
      {total !== null && total > 0 && (
        <>
          <progress
            className={styles.bar}
            aria-label={t.progressLabel}
            max={total}
            value={Math.min(progress.downloadedBytes, total)}
          />
          <p className={styles.muted}>
            {t.bytes(formatBytes(progress.downloadedBytes), formatBytes(total))}
            {progress.bytesPerSecond !== null &&
              ` · ${t.speed(formatBytes(progress.bytesPerSecond))}`}
          </p>
        </>
      )}
      {progress.totalFiles !== null && (
        <p className={styles.muted}>{t.files(progress.verifiedFiles, progress.totalFiles)}</p>
      )}
    </div>
  );
}

function ResultView({ result }: { result: OperationResult }) {
  switch (result.outcome) {
    case 'succeeded':
      return (
        <Notice tone="info">
          {t.result.succeeded(
            result.kind,
            result.downloadedFiles,
            formatBytes(result.networkBytes),
            result.reusedFiles,
          )}
        </Notice>
      );
    case 'cancelled':
      return <Notice tone="warning">{t.result.cancelled}</Notice>;
    case 'interrupted':
      return <Notice tone="warning">{t.result.interrupted}</Notice>;
    case 'failed':
      return <Notice tone="danger">{result.error?.message ?? t.result.interrupted}</Notice>;
  }
}

/** Install, retry, repair and cancel for one installation, with live progress. */
export function InstallPanel({
  instance,
  activeOperation,
  progress,
  recentOperations,
}: InstallPanelProps) {
  const install = useCommand((requestId: string, id: string) => ipc.installInstance(id, requestId));
  const repair = useCommand((requestId: string, id: string) => ipc.repairInstance(id, requestId));
  const cancel = useCommand((requestId: string, operationId: string) =>
    ipc.cancelOperation(operationId, requestId),
  );
  const mine = activeOperation?.instanceId === instance.id ? activeOperation : null;
  const otherBusy = activeOperation !== null && mine === null;
  const ownProgress = mine !== null && progress?.operationId === mine.operationId ? progress : null;
  const last = recentOperations.find((r) => r.instanceId === instance.id) ?? null;
  const health = instance.health;
  const unfinished =
    last !== null &&
    last.kind === 'install' &&
    last.outcome !== 'succeeded' &&
    health === 'not_installed';
  const error = install.error ?? repair.error ?? cancel.error;

  return (
    <Panel title={t.title}>
      <p className={styles.muted}>{t.explain[health]}</p>
      {mine !== null && (
        <>
          {ownProgress !== null ? (
            <ProgressView progress={ownProgress} />
          ) : (
            <p role="status">{t.phase.resolving}</p>
          )}
          <div className={forms.actions}>
            <Button
              disabled={cancel.pending || ownProgress?.phase === 'cancel_requested'}
              onClick={() => void cancel.run(mine.operationId)}
            >
              {ownProgress?.phase === 'cancel_requested' ? t.cancelling : t.cancel}
            </Button>
          </div>
        </>
      )}
      {mine === null && last !== null && <ResultView result={last} />}
      {otherBusy && <Notice tone="info">{t.otherBusy}</Notice>}
      {mine === null && health !== 'unsupported' && (
        <div className={forms.actions}>
          {health === 'not_installed' ? (
            <Button
              variant="primary"
              disabled={otherBusy || install.pending}
              onClick={() => void install.run(instance.id)}
            >
              {unfinished ? t.retry : t.install}
            </Button>
          ) : (
            <Button
              variant={health === 'ready' ? 'secondary' : 'primary'}
              disabled={otherBusy || repair.pending}
              onClick={() => void repair.run(instance.id)}
            >
              {health === 'ready' ? t.verify : t.repair}
            </Button>
          )}
        </div>
      )}
      {health !== 'not_installed' && <p className={styles.muted}>{t.userData}</p>}
      {error !== null && <Notice tone="danger">{error}</Notice>}
    </Panel>
  );
}
