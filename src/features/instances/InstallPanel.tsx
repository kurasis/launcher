import { Button } from '../../components/Button';
import { Notice } from '../../components/Notice';
import { Panel } from '../../components/Panel';
import forms from '../../components/forms.module.css';
import type {
  AccountState,
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
import { localizeError } from '../../lib/strings';
import { gameLabel } from './loaderLabel';

interface InstallPanelProps {
  instance: InstanceSummary;
  /** Without verified access, Play uses the no-token profile instead (D-033). */
  accountState: AccountState;
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
  accountState,
  activeOperation,
  progress,
  recentOperations,
}: InstallPanelProps) {
  const install = useCommand((requestId: string, id: string) => ipc.installInstance(id, requestId));
  const repair = useCommand((requestId: string, id: string) => ipc.repairInstance(id, requestId));
  const play = useCommand((requestId: string, id: string, notoken: boolean) =>
    ipc.launchGame(id, requestId, notoken),
  );
  // An account that is (being re)checked plays in full; any other state plays
  // without a token (Minecraft's demo).
  const notoken = accountState !== 'verified_access' && accountState !== 'refreshing';
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
        <div className={styles.heroActions}>
          {health === 'not_installed' ? (
            <Button
              variant="primary"
              size="large"
              disabled={otherBusy || install.pending}
              onClick={() => void install.run(instance.id)}
            >
              {unfinished ? t.retry : t.install}
            </Button>
          ) : (
            <>
              {health === 'ready' && (
                <Button
                  variant="primary"
                  size="large"
                  disabled={otherBusy || play.pending}
                  onClick={() => void play.run(instance.id, notoken)}
                >
                  {play.pending ? t.starting : notoken ? t.playNoToken : t.play}
                </Button>
              )}
              <Button
                variant={health === 'ready' ? 'secondary' : 'primary'}
                disabled={otherBusy || repair.pending}
                onClick={() => void repair.run(instance.id)}
              >
                {health === 'ready' ? t.verify : t.repair}
              </Button>
            </>
          )}
        </div>
      )}
      {mine === null && health === 'ready' && notoken && (
        <p className={styles.muted}>{t.noTokenHint}</p>
      )}
      {health !== 'not_installed' && <p className={styles.muted}>{t.userData}</p>}
      {error !== null && <Notice tone="danger">{error}</Notice>}
    </Panel>
  );
}
