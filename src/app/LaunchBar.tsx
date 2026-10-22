import type { ReactNode } from 'react';
import { Button } from '../components/Button';
import { Icon } from '../components/Icon';
import type { AppSnapshot, InstanceSummary } from '../lib/contracts';
import { progressPercent } from '../lib/format';
import { ipc } from '../lib/ipc';
import { strings } from '../lib/strings';
import { useCommand } from '../lib/useCommand';
import { gameLabel } from '../features/instances/loaderLabel';
import { PlayAs } from '../features/instances/PlayAs';
import styles from './LaunchBar.module.css';

const t = strings.install;

interface LaunchBarProps {
  snapshot: AppSnapshot;
  /** Opens Installations; left out on Installations itself, where the bar links nowhere. */
  onOpenInstallations?: (() => void) | undefined;
}

/**
 * Concept A's launch bar at the foot of every screen (D-072): the chosen installation,
 * who plays it, and its main action (Play, Install or Repair). Progress and the
 * running game show here too; their details stay on Installations.
 */
export function LaunchBar({ snapshot, onOpenInstallations }: LaunchBarProps) {
  const instance =
    snapshot.instances.find((i) => i.id === snapshot.settings.selectedInstanceId) ?? null;
  if (instance === null) {
    return (
      <section className={styles.bar} aria-label={strings.nav.launchBar}>
        <span className={styles.tile} aria-hidden="true" />
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
    />
  );
}

function ChosenBar({
  instance,
  snapshot,
  onOpenInstallations,
}: LaunchBarProps & { instance: InstanceSummary }) {
  const install = useCommand((requestId: string, id: string) => ipc.installInstance(id, requestId));
  const repair = useCommand((requestId: string, id: string) => ipc.repairInstance(id, requestId));
  const play = useCommand((requestId: string, id: string) => ipc.launchGame(id, requestId));
  const active = snapshot.activeOperation;
  const mine = active?.instanceId === instance.id ? active : null;
  const otherBusy = active !== null && mine === null;
  const offline = snapshot.network === 'offline';
  // While something else runs, install and repair wait in the queue (D-049).
  const queued = snapshot.queue.find((q) => q.instanceId === instance.id) ?? null;
  const progress =
    mine !== null && snapshot.operationProgress?.operationId === mine.operationId
      ? snapshot.operationProgress
      : null;
  const percent = progressPercent(progress);
  const running = mine?.kind === 'launch' || snapshot.currentRun?.instanceId === instance.id;
  const health = instance.health;
  const last = snapshot.recentOperations.find((r) => r.instanceId === instance.id) ?? null;
  const unfinished =
    last !== null &&
    last.kind === 'install' &&
    last.outcome !== 'succeeded' &&
    health === 'not_installed';
  const error = play.error ?? install.error ?? repair.error;

  let action: ReactNode = null;
  if (running) {
    action = (
      <p className={styles.state} role="status">
        {t.gameRunning}
      </p>
    );
  } else if (mine !== null) {
    action = (
      <p className={styles.state} role="status">
        {percent !== null
          ? `${strings.activity[mine.kind]} ${strings.activity.percent({ percent })}`
          : strings.activity[mine.kind]}
      </p>
    );
  } else if (health === 'ready') {
    action = (
      <Button
        variant="primary"
        size="large"
        disabled={otherBusy || play.pending}
        onClick={() => void play.run(instance.id)}
      >
        <Icon name="play" size={18} className={styles.playIcon} />
        {play.pending ? t.starting : t.play}
      </Button>
    );
  } else if (health === 'not_installed') {
    action = (
      <Button
        variant="primary"
        size="large"
        disabled={install.pending || queued !== null}
        onClick={() => void install.run(instance.id)}
      >
        {otherBusy || offline ? strings.downloads.addToQueue : unfinished ? t.retry : t.install}
      </Button>
    );
  } else if (health !== 'unsupported') {
    action = (
      <Button
        variant="primary"
        size="large"
        disabled={repair.pending || queued !== null}
        onClick={() => void repair.run(instance.id)}
      >
        {otherBusy || offline ? strings.downloads.addToQueue : t.repair}
      </Button>
    );
  }

  const chosen = (
    <>
      <span className={styles.tile} aria-hidden="true" />
      <span className={styles.what}>
        <span className={styles.name}>{instance.displayName}</span>
        <span className={styles.note}>
          {gameLabel(instance)} · {strings.instances.health[health]}
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
      {queued !== null && (
        <p className={styles.note} role="status">
          {offline ? strings.downloads.waitingForNetwork : strings.downloads.queued}
        </p>
      )}
      <div className={styles.end}>
        {health === 'ready' && !running && mine === null && (
          <PlayAs instance={instance} account={snapshot.account} compact />
        )}
        {action}
      </div>
    </section>
  );
}
