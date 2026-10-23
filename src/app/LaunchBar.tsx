import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Button } from '../components/Button';
import { Icon } from '../components/Icon';
import { InstanceTile } from '../components/InstanceTile';
import { ProgressBar } from '../components/ProgressBar';
import type { ActiveActivity, AppSnapshot, InstanceSummary } from '../lib/contracts';
import { progressPercent } from '../lib/format';
import { ipc } from '../lib/ipc';
import { strings } from '../lib/strings';
import { useCommand } from '../lib/useCommand';
import { detailSentences, percentTail, stateLine } from '../features/downloads/operationText';
import { gameLabel } from '../features/instances/loaderLabel';
import { mainActionOf } from '../features/instances/mainAction';
import { PlayAs } from '../features/instances/PlayAs';
import styles from './LaunchBar.module.css';

/** A request to run an installation's main action in the bar, e.g. from the command palette. */
export interface LaunchRequest {
  instanceId: string;
  /** When it was asked (Date.now()): each request runs once. */
  at: number;
}

/** A request the bar has not reached in this long is dropped (its installation was never chosen). */
const REQUEST_LIFETIME_MS = 15_000;

interface LaunchBarProps {
  snapshot: AppSnapshot;
  /** Opens Installations; left out on Installations itself, where the bar links nowhere. */
  onOpenInstallations?: (() => void) | undefined;
  /**
   * Runs the main action of this installation once it is the chosen one, so its
   * pending state, error and progress show here.
   */
  request?: LaunchRequest | undefined;
}

/**
 * Concept A's launch bar at the foot of every screen (D-072): the chosen installation,
 * who plays it, and its main action (Play, Install or Repair). While an install or
 * repair runs it shows that instead (name, phase, bytes, files, a bar and Cancel),
 * unless the chosen installation can still be added to the queue.
 */
export function LaunchBar({ snapshot, onOpenInstallations, request }: LaunchBarProps) {
  // The last request taken, kept here: the chosen bar below remounts with every choice.
  const [handledAt, setHandledAt] = useState<number | null>(null);
  const instance =
    snapshot.instances.find((i) => i.id === snapshot.settings.selectedInstanceId) ?? null;
  if (instance === null) {
    const operation = downloadOf(snapshot);
    if (operation !== null) {
      return <OperationStrip snapshot={snapshot} operation={operation} error={null} />;
    }
    return (
      <section className={styles.bar} aria-label={strings.nav.launchBar}>
        <span className={styles.emptyTile} aria-hidden="true" />
        <p className={styles.what}>
          <span className={styles.name}>{strings.nav.noInstallation}</span>
        </p>
        {onOpenInstallations !== undefined && (
          <Button variant="primary" onClick={onOpenInstallations}>
            {snapshot.instances.length === 0
              ? strings.nav.openInstallations
              : strings.nav.chooseInstallation}
          </Button>
        )}
      </section>
    );
  }
  // Keyed by installation: an error or a pending label never carries over to another one.
  return (
    <ChosenBar
      key={instance.id}
      instance={instance}
      snapshot={snapshot}
      onOpenInstallations={onOpenInstallations}
      request={
        request !== undefined && request.instanceId === instance.id && request.at !== handledAt
          ? request
          : undefined
      }
      onRequestHandled={setHandledAt}
    />
  );
}

function ChosenBar({
  instance,
  snapshot,
  onOpenInstallations,
  request,
  onRequestHandled,
}: LaunchBarProps & {
  instance: InstanceSummary;
  onRequestHandled: (at: number) => void;
}) {
  const install = useCommand((requestId: string, id: string) => ipc.installInstance(id, requestId));
  const repair = useCommand((requestId: string, id: string) => ipc.repairInstance(id, requestId));
  const play = useCommand((requestId: string, id: string) => ipc.launchGame(id, requestId));
  const offline = snapshot.network === 'offline';
  const health = instance.health;
  const error = play.error ?? install.error ?? repair.error;
  const main = mainActionOf(instance, snapshot, {
    play: play.pending,
    install: install.pending,
    repair: repair.pending,
  });
  const runMain = () => {
    if (main.command === 'launch') void play.run(instance.id);
    else if (main.command === 'install') void install.run(instance.id);
    else if (main.command === 'repair') void repair.run(instance.id);
  };

  // A request from elsewhere runs once, while it is fresh and the action can run;
  // `taken` also keeps a development double run of effects from running it twice.
  const taken = useRef<number | null>(null);
  useEffect(() => {
    if (request === undefined || taken.current === request.at) return;
    taken.current = request.at;
    onRequestHandled(request.at);
    if (main.enabled && Date.now() - request.at < REQUEST_LIFETIME_MS) runMain();
  });

  // The install or repair that runs, in place of the chosen installation, unless that
  // one offers Add to queue. The hooks above stay: a request is taken (and dropped)
  // now, never run later.
  const operation = downloadOf(snapshot);
  if (
    operation !== null &&
    (operation.instanceId === instance.id ||
      main.kind === 'play' ||
      main.kind === 'busy' ||
      main.kind === 'none')
  ) {
    return <OperationStrip snapshot={snapshot} operation={operation} error={error} />;
  }

  let action: ReactNode = null;
  if (main.kind === 'running' || main.kind === 'busy') {
    action = (
      <p className={styles.state} role="status">
        {main.label}
      </p>
    );
  } else if (main.kind === 'play') {
    action = (
      <Button variant="primary" size="large" disabled={!main.enabled} onClick={runMain}>
        <Icon name="play" size={18} className={styles.playIcon} />
        {main.label}
      </Button>
    );
  } else if (main.kind !== 'none') {
    action = (
      <Button variant="primary" size="large" disabled={!main.enabled} onClick={runMain}>
        {main.label}
      </Button>
    );
  }

  const chosen = (
    <>
      <InstanceTile id={instance.id} size={40} />
      <span className={styles.what}>
        <span className={styles.name}>{instance.displayName}</span>
        <span className={styles.note}>
          <bdi>{gameLabel(instance)}</bdi> · {strings.instances.health[health]}
        </span>
      </span>
    </>
  );

  return (
    <section className={styles.bar} aria-label={strings.nav.launchBar}>
      {onOpenInstallations !== undefined ? (
        <button
          type="button"
          className={styles.chosen}
          aria-label={`${strings.nav.openInstallations}: ${instance.displayName}, ${gameLabel(instance)} · ${strings.instances.health[health]}`}
          title={strings.nav.openInstallations}
          onClick={onOpenInstallations}
        >
          {chosen}
        </button>
      ) : (
        <div className={styles.chosen}>{chosen}</div>
      )}
      {error !== null && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
      {main.queued && (
        <p className={styles.note} role="status">
          {offline ? strings.downloads.waitingForNetwork : strings.downloads.queued}
        </p>
      )}
      <div className={styles.end}>
        {main.kind === 'play' && <PlayAs instance={instance} account={snapshot.account} compact />}
        {action}
      </div>
    </section>
  );
}

/** The install or repair that runs, if one does. */
function downloadOf(snapshot: AppSnapshot): ActiveActivity | null {
  const active = snapshot.activeOperation;
  return active !== null && (active.kind === 'install' || active.kind === 'repair') ? active : null;
}

/** An install or repair in progress: name, phase, bytes, files, a bar and Cancel. */
function OperationStrip({
  snapshot,
  operation,
  error,
}: {
  snapshot: AppSnapshot;
  operation: ActiveActivity;
  /** What the chosen installation's last command answered. */
  error: string | null;
}) {
  const cancel = useCommand((requestId: string, id: string) => ipc.cancelOperation(id, requestId));
  const progress =
    snapshot.operationProgress?.operationId === operation.operationId
      ? snapshot.operationProgress
      : null;
  const running = snapshot.instances.find((i) => i.id === operation.instanceId);
  const cancelling = progress?.phase === 'cancel_requested';
  const problem = cancel.error ?? error;
  return (
    <section className={styles.bar} aria-label={strings.nav.launchBar}>
      {running === undefined ? (
        <span className={styles.emptyTile} aria-hidden="true" />
      ) : (
        <InstanceTile id={running.id} size={40} />
      )}
      <div className={styles.operation}>
        <p className={styles.line}>
          <span className={styles.name} dir="auto">
            {running?.displayName ?? strings.downloads.unknown}
          </span>
          <span className={styles.phase}>
            {/* Read out when the phase changes, not with every percent. */}
            <span role="status">{stateLine(operation.kind, progress)}</span>
            {percentTail(progress)}
          </span>
          <span className={styles.details}>{detailSentences(progress).join(' ')}</span>
        </p>
        <ProgressBar
          value={progressPercent(progress)}
          label={strings.downloads.progress}
          className={styles.progress}
        />
      </div>
      {problem !== null && (
        <p
          className={[styles.error, styles.stripError].filter(Boolean).join(' ')}
          role="alert"
          title={problem}
        >
          {problem}
        </p>
      )}
      <div className={styles.end}>
        {snapshot.queue.length > 0 && (
          <p className={styles.queue}>
            {strings.downloads.queue({ count: snapshot.queue.length })}
          </p>
        )}
        <Button
          variant="secondary"
          aria-disabled={cancel.pending || cancelling || undefined}
          onClick={() => {
            if (!cancel.pending && !cancelling) void cancel.run(operation.operationId);
          }}
        >
          {cancelling ? strings.install.cancelling : strings.install.cancel}
        </Button>
      </div>
    </section>
  );
}
