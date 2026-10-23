import { useEffect, useId, useRef, useState } from 'react';
import { Button } from '../../components/Button';
import { Icon } from '../../components/Icon';
import { InstanceTile } from '../../components/InstanceTile';
import { ProgressBar } from '../../components/ProgressBar';
import { StatusBadge, type StatusTone } from '../../components/StatusBadge';
import type { AppSnapshot, OperationKind, Outcome } from '../../lib/contracts';
import { formatBytes, progressPercent } from '../../lib/format';
import { ipc } from '../../lib/ipc';
import { strings } from '../../lib/strings';
import { trafficTotal } from '../../lib/traffic';
import { useCommand } from '../../lib/useCommand';
import { gameLabel } from '../instances/loaderLabel';
import { AsideApproval } from '../network/AsideApproval';
import { sourceNames } from '../network/sourceNames';
import { around, detailSentences, percentTail, resultLine, stateLine } from './operationText';
import styles from './DownloadManager.module.css';

const t = strings.downloads;
/** How long each file name stays in the rotating line. */
const ROTATE_MS = 1200;
/** How often the month's traffic is read again while the panel is open (D-051). */
const TRAFFIC_MS = 5000;
const MIB = 1024 * 1024;
/** Operations the download manager follows. */
const DOWNLOADS: OperationKind[] = ['install', 'repair'];
const TONES: Record<Outcome, StatusTone> = {
  succeeded: 'success',
  failed: 'danger',
  cancelled: 'neutral',
  interrupted: 'warning',
};

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
  // Only the name is in mono, left to right; the words around it are the language's.
  const [before, after] = around((value) => t.file({ name: value }));
  return (
    <span>
      {before}
      <span className={styles.file} dir="ltr" key={name}>
        {name}
      </span>
      {after}
    </span>
  );
}

/** "Metered connection: 1.2 GiB of 5.0 GiB downloaded this month", read while it shows. */
function MeteredLine({ limitMib }: { limitMib: number }) {
  const [used, setUsed] = useState<number | null>(null);
  useEffect(() => {
    let live = true;
    const read = () => {
      ipc
        .getTraffic()
        .then((summary) => {
          if (live) setUsed(trafficTotal(summary.thisMonth));
        })
        .catch(() => {
          // Settings → Network says why; here the line just waits for the next read.
        });
    };
    read();
    const timer = setInterval(read, TRAFFIC_MS);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, []);
  if (used === null) return null;
  return (
    <p className={styles.hint}>
      {limitMib > 0
        ? t.metered({ used: formatBytes(used), limit: formatBytes(limitMib * MIB) })
        : t.meteredNoLimit({ used: formatBytes(used) })}
    </p>
  );
}

/** An installation's tile, or an empty outline once it was removed. */
function Tile({ id }: { id: string | null }) {
  return id === null ? (
    <span className={styles.noTile} aria-hidden="true" />
  ) : (
    <InstanceTile id={id} size={32} />
  );
}

/**
 * The download manager (D-049): a row of the rail with the percent of the running
 * install or repair, and a panel beside the rail with it (phase, bar, bytes, speed,
 * files and the names of files being fetched), the queue waiting behind it, recent
 * results and, on a metered connection, the month's traffic.
 */
export function DownloadManager({ snapshot }: { snapshot: AppSnapshot }) {
  const [open, setOpen] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const headingId = useId();
  const cancel = useCommand((requestId: string, id: string) => ipc.cancelOperation(id, requestId));
  const active =
    snapshot.activeOperation !== null && DOWNLOADS.includes(snapshot.activeOperation.kind)
      ? snapshot.activeOperation
      : null;
  const progress =
    active !== null && snapshot.operationProgress?.operationId === active.operationId
      ? snapshot.operationProgress
      : null;
  const percent = progressPercent(progress);
  const find = (id: string | null) => snapshot.instances.find((i) => i.id === id);
  const tileOf = (id: string | null) => (find(id) === undefined ? null : id);
  const name = (id: string | null) => find(id)?.displayName ?? t.unknown;
  // Installations often share a name; then the version tells them apart.
  const shared = (id: string | null) => {
    const instance = find(id);
    if (instance === undefined) return null;
    const same = snapshot.instances.filter((i) => i.displayName === instance.displayName);
    return same.length > 1 ? gameLabel(instance) : null;
  };
  const queue = snapshot.queue;
  const recent = snapshot.recentOperations.slice(0, 3);
  const offline = snapshot.network === 'offline';
  // A download that waits for permission asks in a dialog of its own (D-051); the
  // row shows it waits.
  const asking = progress?.approval ? progress.operationId : null;
  const cancelling = progress?.phase === 'cancel_requested';
  const label = percent === null ? t.title : `${t.title} ${t.percent({ percent })}`;
  const details = detailSentences(progress);

  useEffect(() => {
    if (open) headingRef.current?.focus();
  }, [open]);
  // Closing puts the focus back on the row.
  const close = () => {
    setOpen(false);
    buttonRef.current?.focus();
  };
  const stop = (id: string) => {
    if (!cancel.pending) void cancel.run(id);
  };

  const nameLine = (id: string | null) => {
    const version = shared(id);
    return (
      <>
        {name(id)}
        {version !== null && (
          <span className={styles.version}>
            {' '}
            <bdi>{version}</bdi>
          </span>
        )}
      </>
    );
  };

  return (
    <div
      className={styles.slot}
      onKeyDown={(event) => {
        if (event.key === 'Escape' && open && !event.defaultPrevented) close();
      }}
    >
      {open && (
        <section className={styles.panel} aria-labelledby={headingId}>
          <div className={styles.head}>
            <h2 id={headingId} ref={headingRef} className={styles.title} tabIndex={-1}>
              {t.title}
            </h2>
            <button
              type="button"
              className={styles.close}
              aria-label={t.close}
              title={t.close}
              onClick={close}
            >
              <Icon name="x" size={18} />
            </button>
          </div>
          {(snapshot.update.phase === 'downloading' || snapshot.update.phase === 'ready') &&
            snapshot.update.version !== null && (
              <div className={styles.card}>
                <p className={styles.name}>
                  {strings.updates.manager({ version: snapshot.update.version })}
                </p>
                {snapshot.update.phase === 'ready' ? (
                  <p className={styles.hint}>{strings.updates.readyShort}</p>
                ) : (
                  <ProgressBar
                    value={snapshot.update.downloaded}
                    max={snapshot.update.total ?? undefined}
                    label={strings.updates.manager({ version: snapshot.update.version })}
                  />
                )}
              </div>
            )}
          {snapshot.mirroredHosts.length > 0 && (
            <p className={styles.hint} role="status">
              {strings.mirror.inUse({ sites: sourceNames(snapshot.mirroredHosts).join(', ') })}
            </p>
          )}
          {active === null && queue.length === 0 && <p className={styles.hint}>{t.none}</p>}
          {active !== null && (
            <div className={styles.card}>
              <div className={styles.row}>
                <Tile id={tileOf(active.instanceId)} />
                <div className={styles.what}>
                  <p className={styles.name}>{nameLine(active.instanceId)}</p>
                  <p className={styles.hint}>
                    {stateLine(active.kind, progress)}
                    {percentTail(progress)}
                  </p>
                </div>
                <Button
                  variant="secondary"
                  size="small"
                  aria-disabled={cancel.pending || cancelling || undefined}
                  onClick={() => {
                    if (!cancelling) stop(active.operationId);
                  }}
                >
                  {cancelling ? strings.install.cancelling : t.cancel}
                </Button>
              </div>
              <ProgressBar value={percent} label={t.progress} />
              {(details.length > 0 || (progress?.currentFiles.length ?? 0) > 0) && (
                <p className={[styles.hint, styles.numbers].filter(Boolean).join(' ')}>
                  {details.join(' ')}
                  {details.length > 0 && progress !== null && progress.currentFiles.length > 0
                    ? ' '
                    : null}
                  {progress !== null && <RotatingName names={progress.currentFiles} />}
                </p>
              )}
              {asking !== null && <AsideApproval operationId={asking} />}
            </div>
          )}
          {queue.length > 0 && (
            <div className={styles.group}>
              <h3 className={styles.label}>{t.queue({ count: queue.length })}</h3>
              <ol className={styles.list}>
                {queue.map((item) => (
                  <li key={item.queueId} className={styles.row}>
                    <Tile id={tileOf(item.instanceId)} />
                    <div className={styles.what}>
                      <p className={styles.name}>{nameLine(item.instanceId)}</p>
                      <p className={styles.hint}>
                        {offline
                          ? t.waitsForNetwork({ kind: item.kind })
                          : t.startsAfter({ kind: item.kind })}
                      </p>
                    </div>
                    <Button
                      variant="secondary"
                      size="small"
                      aria-label={t.removeNamed({ name: name(item.instanceId) })}
                      aria-disabled={cancel.pending || undefined}
                      onClick={() => {
                        stop(item.queueId);
                      }}
                    >
                      {t.remove}
                    </Button>
                  </li>
                ))}
              </ol>
            </div>
          )}
          {recent.length > 0 && (
            <div className={styles.group}>
              <h3 className={styles.label}>{t.recent}</h3>
              <ul className={styles.list}>
                {recent.map((result) => (
                  <li key={result.operationId} className={styles.row}>
                    <Tile id={tileOf(result.instanceId)} />
                    <div className={styles.what}>
                      <p className={styles.nameRow}>
                        <span className={styles.name}>{nameLine(result.instanceId)}</span>
                        <StatusBadge
                          tone={TONES[result.outcome]}
                          label={strings.diagnostics.outcome[result.outcome]}
                          dot={false}
                        />
                      </p>
                      <p className={[styles.hint, styles.clamp].filter(Boolean).join(' ')}>
                        {resultLine(result)}
                      </p>
                    </div>
                  </li>
                ))}
              </ul>
            </div>
          )}
          {cancel.error !== null && (
            <p className={styles.error} role="alert">
              {cancel.error}
            </p>
          )}
          {snapshot.settings.metered && (
            <MeteredLine limitMib={snapshot.settings.monthlyLimitMib} />
          )}
        </section>
      )}
      <button
        ref={buttonRef}
        type="button"
        className={styles.railButton}
        data-working={active !== null && asking === null ? 'true' : undefined}
        data-asking={asking !== null ? 'true' : undefined}
        aria-label={label}
        aria-expanded={open}
        title={label}
        onClick={() => {
          if (open) close();
          else setOpen(true);
        }}
      >
        <Icon name="downloads" className={styles.icon} />
        <span className={styles.railLabel} aria-hidden="true">
          {label}
        </span>
        {queue.length > 0 && <span className={styles.badge}>{queue.length}</span>}
      </button>
    </div>
  );
}
