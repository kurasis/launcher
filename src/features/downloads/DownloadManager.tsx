import { useEffect, useRef, useState } from 'react';
import type { AppSnapshot, OperationKind } from '../../lib/contracts';
import { formatBytes } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { useCommand } from '../../lib/useCommand';
import { gameLabel } from '../instances/loaderLabel';
import { DownloadApproval } from '../network/DownloadApproval';
import { sourceNames } from '../network/sourceNames';
import styles from './DownloadManager.module.css';

const t = strings.downloads;
/** How long each file name stays in the rotating line. */
const ROTATE_MS = 1200;
/** Operations the download manager follows. */
const DOWNLOADS: OperationKind[] = ['install', 'repair'];

/**
 * Shows the files being downloaded one after another. The name changes only on
 * its own beat, not with every progress update, so each one can be read.
 */
function RotatingName({ names }: { names: string[] }) {
  const latest = useRef(names);
  const [shown, setShown] = useState<string | null>(names[0] ?? null);
  useEffect(() => {
    latest.current = names;
  }, [names]);
  useEffect(() => {
    const timer = setInterval(() => {
      setShown((current) => {
        const list = latest.current;
        if (list.length === 0) return null;
        const index = current === null ? -1 : list.indexOf(current);
        return list[(index + 1) % list.length] ?? null;
      });
    }, ROTATE_MS);
    return () => {
      clearInterval(timer);
    };
  }, []);
  if (names.length === 0) return null;
  const name = shown ?? names[0] ?? '';
  return (
    <p className={styles.file} dir="ltr" key={name}>
      {t.file({ name })}
    </p>
  );
}

/**
 * The download manager (D-049): an always visible corner button that animates
 * while files download, and a panel with the running install or repair (phase,
 * percent, speed, files and the names of files being fetched), the queue
 * waiting behind it, and recent results.
 */
export function DownloadManager({ snapshot }: { snapshot: AppSnapshot }) {
  const [open, setOpen] = useState(false);
  const cancel = useCommand((requestId: string, id: string) => ipc.cancelOperation(id, requestId));
  const active =
    snapshot.activeOperation !== null && DOWNLOADS.includes(snapshot.activeOperation.kind)
      ? snapshot.activeOperation
      : null;
  const progress =
    active !== null && snapshot.operationProgress?.operationId === active.operationId
      ? snapshot.operationProgress
      : null;
  const total = progress?.totalDownloadBytes ?? null;
  const ratio = (done: number, all: number | null) =>
    all !== null && all > 0 ? Math.min(100, Math.floor((done / all) * 100)) : null;
  // Bytes while downloading; checked files while checking what is present.
  const percent =
    progress === null
      ? null
      : progress.phase === 'checking_files'
        ? ratio(progress.verifiedFiles, progress.totalFiles)
        : ratio(progress.downloadedBytes, total);
  const details =
    progress === null
      ? []
      : [
          total !== null && total > 0
            ? t.bytes({
                done: formatBytes(progress.downloadedBytes),
                total: formatBytes(total),
              })
            : null,
          progress.bytesPerSecond !== null
            ? t.speed({ rate: formatBytes(progress.bytesPerSecond) })
            : null,
          progress.totalFiles !== null
            ? t.files({ done: progress.verifiedFiles, total: progress.totalFiles })
            : null,
        ].filter((part) => part !== null);
  const find = (id: string | null) => snapshot.instances.find((i) => i.id === id);
  const name = (id: string | null) => find(id)?.displayName ?? t.unknown;
  // Installations often share a name; the version tells them apart.
  const version = (id: string | null) => {
    const instance = find(id);
    return instance === undefined ? null : gameLabel(instance);
  };
  const about = (id: string | null, kind: OperationKind) =>
    [version(id), strings.diagnostics.kind[kind]].filter((part) => part !== null).join(' · ');
  const queue = snapshot.queue;
  const recent = snapshot.recentOperations.slice(0, 3);
  const working = active !== null;
  // A download that waits for permission opens the panel until closed (D-051).
  const [closedFor, setClosedFor] = useState<string | null>(null);
  const asking = progress?.approval ? progress.operationId : null;
  const shown = open || (asking !== null && closedFor !== asking);
  const close = () => {
    setOpen(false);
    setClosedFor(asking);
  };

  return (
    <div
      className={styles.corner}
      onKeyDown={(event) => {
        if (event.key === 'Escape') close();
      }}
    >
      {shown && (
        <section className={styles.panel} aria-label={t.title}>
          <h2 className={styles.title}>{t.title}</h2>
          {snapshot.mirroredHosts.length > 0 && (
            <p className={styles.muted} role="status">
              {strings.mirror.inUse({ sites: sourceNames(snapshot.mirroredHosts).join(', ') })}
            </p>
          )}
          {active === null && queue.length === 0 && <p className={styles.muted}>{t.none}</p>}
          {active !== null && (
            <div className={styles.item}>
              <p className={styles.itemHead}>
                <strong>{name(active.instanceId)}</strong>
                <span className={styles.muted}>{strings.diagnostics.kind[active.kind]}</span>
              </p>
              <p className={styles.muted}>{version(active.instanceId)}</p>
              <p className={styles.muted}>
                {progress === null
                  ? strings.install.phase.resolving
                  : strings.install.phase[progress.phase]}
                {percent !== null && ` · ${t.percent({ percent })}`}
              </p>
              <progress
                className={styles.bar}
                aria-label={t.progress}
                max={100}
                value={percent ?? undefined}
              />
              {details.length > 0 && <p className={styles.muted}>{details.join(' · ')}</p>}
              {progress !== null && <RotatingName names={progress.currentFiles} />}
              {progress?.approval && (
                <DownloadApproval operationId={progress.operationId} approval={progress.approval} />
              )}
              <button
                type="button"
                className={styles.link}
                disabled={cancel.pending || progress?.phase === 'cancel_requested'}
                onClick={() => void cancel.run(active.operationId)}
              >
                {t.cancel}
              </button>
            </div>
          )}
          {queue.length > 0 && (
            <>
              <h3 className={styles.subtitle}>{t.queue({ count: queue.length })}</h3>
              {snapshot.network === 'offline' && (
                <p className={styles.muted}>{t.waitingForNetwork}</p>
              )}
              <ol className={styles.queue}>
                {queue.map((item) => (
                  <li key={item.queueId} className={styles.queued}>
                    <span>
                      <strong>{name(item.instanceId)}</strong>{' '}
                      <span className={styles.muted}>{about(item.instanceId, item.kind)}</span>
                    </span>
                    <button
                      type="button"
                      className={styles.link}
                      disabled={cancel.pending}
                      onClick={() => void cancel.run(item.queueId)}
                    >
                      {t.remove}
                    </button>
                  </li>
                ))}
              </ol>
            </>
          )}
          {recent.length > 0 && (
            <>
              <h3 className={styles.subtitle}>{t.recent}</h3>
              <ul className={styles.queue}>
                {recent.map((result) => (
                  <li key={result.operationId} className={styles.queued}>
                    <span>
                      <strong>{name(result.instanceId)}</strong>{' '}
                      <span className={styles.muted}>{about(result.instanceId, result.kind)}</span>
                    </span>
                    <span className={styles.result} data-outcome={result.outcome}>
                      {strings.diagnostics.outcome[result.outcome]}
                    </span>
                  </li>
                ))}
              </ul>
            </>
          )}
          {cancel.error !== null && <p className={styles.error}>{cancel.error}</p>}
        </section>
      )}
      <button
        type="button"
        className={styles.button}
        data-working={working && asking === null ? 'true' : undefined}
        data-asking={asking !== null ? 'true' : undefined}
        aria-label={t.open}
        aria-expanded={shown}
        title={t.open}
        style={
          percent !== null
            ? ({ '--progress': `${String(percent)}%` } as React.CSSProperties)
            : undefined
        }
        onClick={() => {
          if (shown) close();
          else setOpen(true);
        }}
      >
        <svg className={styles.icon} viewBox="0 0 24 24" aria-hidden="true">
          <path className={styles.arrow} d="M12 3v11m0 0-4.5-4.5M12 14l4.5-4.5" />
          <path d="M4 15v3.5A2.5 2.5 0 0 0 6.5 21h11a2.5 2.5 0 0 0 2.5-2.5V15" />
        </svg>
        {queue.length > 0 && <span className={styles.badge}>{queue.length}</span>}
      </button>
    </div>
  );
}
