import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import forms from '../../components/forms.module.css';
import type {
  AccountSummary,
  ActiveActivity,
  InstanceSummary,
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
import { gameLabel } from './loaderLabel';
import { PlayAs } from './PlayAs';

interface InstallPanelProps {
  instance: InstanceSummary;
  /** Accounts and profiles: who plays this installation (D-045). */
  account: AccountSummary;
  activeOperation: ActiveActivity | null;
  progress: OperationProgress | null;
  recentOperations: OperationResult[];
  /** Installs and repairs waiting their turn (D-049). */
  queue?: QueuedOperation[];
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

/** Install, retry, repair and cancel for one installation, with live progress. */
export function InstallPanel({
  instance,
  account,
  activeOperation,
  progress,
  recentOperations,
  queue = [],
}: InstallPanelProps) {
  const install = useCommand((requestId: string, id: string) => ipc.installInstance(id, requestId));
  const repair = useCommand((requestId: string, id: string) => ipc.repairInstance(id, requestId));
  const play = useCommand((requestId: string, id: string) => ipc.launchGame(id, requestId));
  const cancel = useCommand((requestId: string, operationId: string) =>
    ipc.cancelOperation(operationId, requestId),
  );
  const mine = activeOperation?.instanceId === instance.id ? activeOperation : null;
  const otherBusy = activeOperation !== null && mine === null;
  // While something else runs, install and repair wait in the queue (D-049).
  const queued = queue.find((q) => q.instanceId === instance.id) ?? null;
  const ownProgress = mine !== null && progress?.operationId === mine.operationId ? progress : null;
  const last = recentOperations.find((r) => r.instanceId === instance.id) ?? null;
  const health = instance.health;
  const unfinished =
    last !== null &&
    last.kind === 'install' &&
    last.outcome !== 'succeeded' &&
    health === 'not_installed';
  const error = play.error ?? install.error ?? repair.error ?? cancel.error;

  return (
    <Panel
      variant="hero"
      title={instance.displayName}
      aside={<span className={styles.version}>{gameLabel(instance)}</span>}
    >
      <p className={styles.muted}>{t.explain[health]}</p>
      {instance.loader !== null && health === 'not_installed' && (
        <p className={styles.muted}>
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
      {queued !== null ? (
        <Notice tone="info">
          <p>{strings.downloads.queued}</p>
          <div className={forms.actions}>
            <Button disabled={cancel.pending} onClick={() => void cancel.run(queued.queueId)}>
              {strings.downloads.remove}
            </Button>
          </div>
        </Notice>
      ) : (
        otherBusy && <Notice tone="info">{t.otherBusy}</Notice>
      )}
      {mine === null && health !== 'unsupported' && (
        <div className={styles.heroActions}>
          {health === 'not_installed' ? (
            <Button
              variant="primary"
              size="large"
              disabled={install.pending || queued !== null}
              onClick={() => void install.run(instance.id)}
            >
              {otherBusy ? strings.downloads.addToQueue : unfinished ? t.retry : t.install}
            </Button>
          ) : (
            <>
              {health === 'ready' && (
                <Button
                  variant="primary"
                  size="large"
                  disabled={otherBusy || play.pending}
                  onClick={() => void play.run(instance.id)}
                >
                  {play.pending ? t.starting : t.play}
                </Button>
              )}
              <Button
                variant={health === 'ready' ? 'secondary' : 'primary'}
                disabled={repair.pending || queued !== null}
                onClick={() => void repair.run(instance.id)}
              >
                {otherBusy && health !== 'ready'
                  ? strings.downloads.addToQueue
                  : health === 'ready'
                    ? t.verify
                    : t.repair}
              </Button>
            </>
          )}
        </div>
      )}
      {health === 'ready' && <PlayAs instance={instance} account={account} />}
      {health !== 'not_installed' && <p className={styles.muted}>{t.userData}</p>}
      {error !== null && <Notice tone="danger">{error}</Notice>}
    </Panel>
  );
}
