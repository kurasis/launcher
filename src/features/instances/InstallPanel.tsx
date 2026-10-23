import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import forms from '../../components/forms.module.css';
import type {
  ActiveActivity,
  InstanceSummary,
  NetworkStatus,
  OperationProgress,
  OperationResult,
  QueuedOperation,
} from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import styles from './Instances.module.css';
import { localizeError } from '../../lib/strings';
import { AsideApproval } from '../network/AsideApproval';

interface InstallPanelProps {
  instance: InstanceSummary;
  activeOperation: ActiveActivity | null;
  progress: OperationProgress | null;
  recentOperations: OperationResult[];
  /** Installs and repairs waiting their turn (D-049). */
  queue?: QueuedOperation[];
  /** Offline, installs and repairs wait for the connection (D-052). */
  network?: NetworkStatus;
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
            {t.bytes({ done: formatBytes(progress.downloadedBytes), total: formatBytes(total) })}
            {progress.bytesPerSecond !== null &&
              ` · ${t.speed({ rate: formatBytes(progress.bytesPerSecond) })}`}
          </p>
        </>
      )}
      {progress.totalFiles !== null && (
        <p className={styles.muted}>
          {t.files({ done: progress.verifiedFiles, total: progress.totalFiles })}
        </p>
      )}
    </div>
  );
}

function ResultView({ result }: { result: OperationResult }) {
  switch (result.outcome) {
    case 'succeeded':
      return (
        <Notice tone="info">
          {t.result.succeeded({
            kind: result.kind,
            files: result.downloadedFiles,
            bytes: formatBytes(result.networkBytes),
            reused: result.reusedFiles,
          })}
        </Notice>
      );
    case 'cancelled':
      return <Notice tone="warning">{t.result.cancelled}</Notice>;
    case 'interrupted':
      return <Notice tone="warning">{t.result.interrupted}</Notice>;
    case 'failed':
      return (
        <Notice tone="danger">
          {result.error ? localizeError(result.error) : t.result.interrupted}
        </Notice>
      );
  }
}

/**
 * One installation's state, progress, queue and last result, with Cancel. It sits in
 * the chosen installation's panel under its toolbar (Verify files is there); Play,
 * Install, Repair and "Play as" live in the launch bar (D-072).
 */
export function InstallPanel({
  instance,
  activeOperation,
  progress,
  recentOperations,
  queue = [],
  network = 'online',
}: InstallPanelProps) {
  const cancel = useCommand((requestId: string, operationId: string) =>
    ipc.cancelOperation(operationId, requestId),
  );
  const mine = activeOperation?.instanceId === instance.id ? activeOperation : null;
  const otherBusy = activeOperation !== null && mine === null;
  const offline = network === 'offline';
  // While something else runs, install and repair wait in the queue (D-049).
  const queued = queue.find((q) => q.instanceId === instance.id) ?? null;
  const ownProgress = mine !== null && progress?.operationId === mine.operationId ? progress : null;
  const last = recentOperations.find((r) => r.instanceId === instance.id) ?? null;
  const health = instance.health;

  return (
    <div className={styles.state}>
      <p className={styles.explain}>
        {t.explain[health]}
        {health !== 'not_installed' && ` ${t.userData}`}
      </p>
      {instance.loader !== null && health === 'not_installed' && (
        <p className={styles.explain}>
          {strings.loaders.sources({ loader: strings.loaders.names[instance.loader.kind] })}
        </p>
      )}
      {mine?.kind === 'launch' && <p role="status">{t.gameRunning}</p>}
      {mine !== null && mine.kind !== 'launch' && (
        <>
          {ownProgress !== null ? (
            <ProgressView progress={ownProgress} />
          ) : (
            <p role="status">{t.phase.resolving}</p>
          )}
          <div className={forms.actions}>
            {ownProgress?.approval && (
              <AsideApproval operationId={ownProgress.operationId} size="normal" />
            )}
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
      {offline && mine === null && <Notice tone="warning">{t.offline}</Notice>}
      {queued !== null ? (
        <Notice tone="info">
          <p>{offline ? strings.downloads.waitingForNetwork : strings.downloads.queued}</p>
          <div className={forms.actions}>
            <Button disabled={cancel.pending} onClick={() => void cancel.run(queued.queueId)}>
              {strings.downloads.remove}
            </Button>
          </div>
        </Notice>
      ) : (
        otherBusy && <Notice tone="info">{t.otherBusy}</Notice>
      )}
      {cancel.error !== null && <Notice tone="danger">{cancel.error}</Notice>}
    </div>
  );
}
